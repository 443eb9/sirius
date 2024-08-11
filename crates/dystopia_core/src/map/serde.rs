use std::{fs::File, io::Write, path::Path};

use bevy::{
    app::{App, Plugin, Update},
    asset::{
        io::{Reader, Writer},
        processor::LoadAndSave,
        saver::{AssetSaver, SavedAsset},
        Asset, AssetApp, AssetLoader, AssetServer, Assets, AsyncReadExt, AsyncWriteExt, Handle,
        LoadContext,
    },
    color::{ColorToComponents, LinearRgba},
    log::{error, info},
    math::IVec3,
    prelude::{
        in_state, Commands, Entity, IntoSystemConfigs, OnInsert, Query, Res, ResMut, Trigger,
    },
    reflect::TypePath,
};
use bincode::{
    config::Configuration,
    error::{DecodeError, EncodeError},
    Decode, Encode,
};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use thiserror::Error;

use crate::{
    cosmos::celestial::{BodyIndex, ToLoadTilemap, ToSaveTilemap},
    map::{
        bundle::TilemapBundle,
        storage::ChunkedStorage,
        tilemap::{
            Tile, TileRenderSize, TilemapAnimations, TilemapStorage, TilemapTexture,
            TilemapTextureDescriptor, TilemapTilesets, TilemapTint,
        },
    },
    schedule::state::GameState,
    simulation::SaveName,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const ENCDEC_CONFIG: Configuration = bincode::config::standard();

pub(super) struct TilemapSerdePlugin;

impl Plugin for TilemapSerdePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, load_tilemap.run_if(in_state(GameState::Simulate)))
            .init_asset::<BinaryTilemap>()
            .init_asset_loader::<BinaryTilemapLoader>()
            .register_asset_processor::<LoadAndSave<BinaryTilemapLoader, BinaryTilemapSaver>>(
                BinaryTilemapSaver.into(),
            )
            .observe(save_tilemap);
    }
}

#[derive(Encode, Decode)]
enum BinaryAtlasIndex {
    Static {
        texture: u32,
        atlas: u32,
        flip: u32,
    },
    Animated {
        start: usize,
        len: usize,
        offset_milisec: u32,
    },
}

#[derive(Encode, Decode)]
struct BinaryTilesets {
    size: [u32; 2],
    filter_mode: u32,
    textures: Vec<(String, [u32; 2], [u32; 2])>,
}

#[derive(Encode, Decode)]
struct BinaryTile {
    indices: ([i32; 3], ([i32; 3], usize)),
    atlas: BinaryAtlasIndex,
    tint: [f32; 4],
    visible: bool,
}

// TODO replace `[number; dimension]`s with glam vectors.
#[derive(Encode, Decode, Asset, TypePath)]
pub struct BinaryTilemap {
    version: u32,
    target_body: usize,
    tile_render_size: [f32; 2],
    chunk_size: u32,
    storgae: Vec<([i32; 3], Vec<Option<BinaryTile>>)>,
    tint: [f32; 4],
    tilesets: BinaryTilesets,
    animations: Vec<u32>,
}

#[derive(Error, Debug)]
pub enum TilemapBinaryLoadError {
    #[error("Io error: {0:?}")]
    Io(std::io::Error),
    #[error("Decode error: {0:?}")]
    Decode(DecodeError),
}

#[derive(Default)]
pub struct BinaryTilemapLoader;

impl AssetLoader for BinaryTilemapLoader {
    type Asset = BinaryTilemap;

    type Settings = ();

    type Error = TilemapBinaryLoadError;

    async fn load<'a>(
        &'a self,
        reader: &'a mut Reader<'_>,
        _settings: &'a Self::Settings,
        _load_context: &'a mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .await
            .map_err(|e| TilemapBinaryLoadError::Io(e))?;
        bincode::decode_from_slice(&buf, ENCDEC_CONFIG)
            .map(|r| r.0)
            .map_err(|e| TilemapBinaryLoadError::Decode(e))
    }

    fn extensions(&self) -> &[&str] {
        &["tmb"]
    }
}

#[derive(Error, Debug)]
pub enum TilemapBinarySaveError {
    #[error("Io error: {0:?}")]
    Io(std::io::Error),
    #[error("Encode error: {0:?}")]
    Encode(EncodeError),
}

#[derive(Default)]
pub struct BinaryTilemapSaver;

impl AssetSaver for BinaryTilemapSaver {
    type Asset = BinaryTilemap;

    type Settings = ();

    type OutputLoader = BinaryTilemapLoader;

    type Error = TilemapBinarySaveError;

    async fn save<'a>(
        &'a self,
        writer: &'a mut Writer,
        asset: SavedAsset<'a, Self::Asset>,
        _settings: &'a Self::Settings,
    ) -> Result<<Self::OutputLoader as AssetLoader>::Settings, Self::Error> {
        writer
            .write_all(
                &bincode::encode_to_vec(asset.get(), ENCDEC_CONFIG)
                    .map_err(|e| TilemapBinarySaveError::Encode(e))?,
            )
            .await
            .map_err(|e| TilemapBinarySaveError::Io(e))?;

        Ok(())
    }
}

fn save_tilemap(
    trigger: Trigger<OnInsert, ToSaveTilemap>,
    mut commands: Commands,
    to_unload_query: Query<(
        Entity,
        &BodyIndex,
        &TileRenderSize,
        &TilemapStorage,
        &TilemapTint,
        &TilemapTilesets,
        &TilemapAnimations,
        &ToSaveTilemap,
    )>,
    asset_server: Res<AssetServer>,
    save_name: Res<SaveName>,
) {
    let Ok((
        entity,
        body_index,
        tile_render_size,
        storage,
        tint,
        tilesets,
        animations,
        save_options,
    )) = to_unload_query.get(trigger.entity())
    else {
        return;
    };

    commands.entity(entity).remove::<ToSaveTilemap>();

    let binary = BinaryTilemap {
        version: VERSION.split('.').nth(0).unwrap().parse().unwrap(),
        target_body: **body_index,
        tile_render_size: tile_render_size.to_array(),
        chunk_size: storage.chunk_size(),
        storgae: unsafe {
            (*storage.as_unsafe_cell_readonly().internal)
                .par_iter()
                .map(|(ci, c)| {
                    (
                        ci.to_array(),
                        c.par_iter()
                            .map(|t| {
                                t.as_ref().map(|t| BinaryTile {
                                    indices: std::mem::transmute(t.index),
                                    atlas: std::mem::transmute(t.atlas_index),
                                    tint: t.tint.to_linear().to_f32_array(),
                                    visible: t.visible,
                                })
                            })
                            .collect(),
                    )
                })
                .collect()
        },
        tint: tint.to_linear().to_f32_array(),
        tilesets: BinaryTilesets {
            size: tilesets.size().to_array(),
            filter_mode: tilesets.filter_mode() as u32,
            textures: tilesets
                .textures()
                .iter()
                .map(|tex| {
                    (
                        asset_server.get_path(&tex.handle).unwrap().to_string(),
                        tex.desc.size.to_array(),
                        tex.desc.tile_size.to_array(),
                    )
                })
                .collect(),
        },
        animations: animations.bytes().clone(),
    };

    match bincode::encode_to_vec(binary, ENCDEC_CONFIG) {
        Ok(data) => {
            let path = Path::new(&std::env::var("PROGRAM_ROOT").unwrap())
                .join("assets")
                .join("data")
                .join("saves")
                .join(&**save_name)
                .join("maps")
                .join(format!("{}.tmb", **body_index));

            // TODO move to standard way after issue #11216 get solved
            match write_bytes(&data, &path) {
                Ok(len) => {
                    if save_options.remove_after_done {
                        commands.entity(entity).remove::<(
                            TileRenderSize,
                            TilemapStorage,
                            TilemapTint,
                            TilemapTilesets,
                            TilemapAnimations,
                        )>();
                    }

                    info!(
                        "Successfully saved tilemap of body {}. {} bytes are written.",
                        **body_index, len
                    );
                }
                Err(err) => {
                    error!(
                        "Failed to write data into tilemap save of body {}: {}",
                        **body_index, err
                    );
                }
            }
        }
        Err(err) => error!(
            "Failed to encode tilemap data for body {}: {}",
            **body_index, err
        ),
    }
}

fn write_bytes(bytes: &[u8], path: &Path) -> Result<usize, std::io::Error> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut file = File::create(path)?;
    file.write(bytes)
}

fn load_tilemap(
    mut commands: Commands,
    to_load_query: Query<(
        Entity,
        &BodyIndex,
        &ToLoadTilemap,
        Option<&Handle<BinaryTilemap>>,
    )>,
    save_name: Res<SaveName>,
    mut binary_tilemap_assets: ResMut<Assets<BinaryTilemap>>,
    asset_server: Res<AssetServer>,
) {
    for (entity, body_index, _load_options, binary_tilemap_handle) in &to_load_query {
        if binary_tilemap_handle.is_none() {
            commands.entity(entity).insert(
                asset_server.load::<BinaryTilemap>(
                    Path::new("data")
                        .join("saves")
                        .join(&**save_name)
                        .join("maps")
                        .join(format!("{}.tmb", **body_index)),
                ),
            );
            continue;
        }

        let Some(binary_tilemap) = binary_tilemap_assets.remove(binary_tilemap_handle.unwrap())
        else {
            return;
        };

        let bundle = TilemapBundle {
            tile_render_size: TileRenderSize(binary_tilemap.tile_render_size.into()),
            storgae: TilemapStorage::from(ChunkedStorage::new_init(
                binary_tilemap.chunk_size,
                binary_tilemap
                    .storgae
                    .into_par_iter()
                    .map(|(ci, c)| {
                        (
                            IVec3::from(ci),
                            c.into_iter()
                                .map(|t| {
                                    t.map(|t| Tile {
                                        index: unsafe { std::mem::transmute(t.indices) },
                                        atlas_index: unsafe { std::mem::transmute(t.atlas) },
                                        tint: LinearRgba::from_f32_array(t.tint).into(),
                                        visible: t.visible,
                                    })
                                })
                                .collect::<Vec<_>>()
                                .into(),
                        )
                    })
                    .collect(),
            )),
            tilesets: TilemapTilesets {
                size: binary_tilemap.tilesets.size.into(),
                filter_mode: unsafe { std::mem::transmute(binary_tilemap.tilesets.filter_mode) },
                textures: binary_tilemap
                    .tilesets
                    .textures
                    .into_iter()
                    .map(|(path, size, tile_size)| TilemapTexture {
                        handle: asset_server.load(path),
                        desc: TilemapTextureDescriptor {
                            size: size.into(),
                            tile_size: tile_size.into(),
                        },
                    })
                    .collect(),
            },
            tint: TilemapTint(LinearRgba::from_f32_array(binary_tilemap.tint).into()),
            animations: unsafe { std::mem::transmute(binary_tilemap.animations) },
            ..Default::default()
        };

        commands.entity(entity).insert(bundle);
    }
}
