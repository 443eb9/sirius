//! Transition between cosmos view to body view
//! - cosmos view: Interface where you will see bodies rotate around.
//! - body view: The tilemap of a specific body.

use bevy::prelude::{
    Commands, NextState, Query, Res, ResMut, Transform, Visibility, With, Without,
};

use crate::{
    body::FocusingOn,
    cosmos::celestial::{BodyIndex, BodyTilemap},
    impl_transition_plugin,
    input::event::{condition::keyboard_event_activating, OPEN_COSMOS_VIEW},
    map::tilemap::TilemapStorage,
    scene::transition::CameraRecoverTransform,
    schedule::state::SceneState,
    sim::{MainCamera, ViewScale},
};

impl_transition_plugin!(
    FocusingBodyTransitionPlugin,
    SceneState::FocusingBody,
    focus_body,
    handle_cosmos_view_entering.run_if(keyboard_event_activating(OPEN_COSMOS_VIEW)),
    unfocus_body
);

fn focus_body(
    mut commands: Commands,
    bodies_query: Query<(&CameraRecoverTransform, &BodyTilemap)>,
    focusing_on: Res<FocusingOn>,
    mut camera_query: Query<&mut Transform, (With<MainCamera>, Without<BodyIndex>)>,
    mut view_scale: ResMut<ViewScale>,
) {
    let (recover_transl, tilemap) = bodies_query.get(focusing_on.entity).unwrap();

    commands.entity(**tilemap).insert(Visibility::Inherited);

    recover_transl.recover(&mut camera_query.single_mut(), &mut view_scale);
}

fn unfocus_body(
    mut commands: Commands,
    mut tilemaps_query: Query<&mut Visibility, With<TilemapStorage>>,
    focusing_on: Res<FocusingOn>,
) {
    *tilemaps_query.get_mut(*focusing_on.tilemap).unwrap() = Visibility::Hidden;
    commands.remove_resource::<FocusingOn>();
}

fn handle_cosmos_view_entering(mut scene_state: ResMut<NextState<SceneState>>) {
    scene_state.set(SceneState::CosmosView);
}
