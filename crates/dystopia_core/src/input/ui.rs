use bevy::{
    input::{mouse::MouseButtonInput, ButtonInput, ButtonState},
    prelude::{
        Commands, Entity, EventReader, EventWriter, GlobalTransform, KeyCode, MouseButton,
        ParallelCommands, Query, Res, ResMut, ViewVisibility, With, Without,
    },
    ui::{Node, Style},
};

use crate::{
    input::{
        Dragable, MouseHovering, MouseInput, RayTransparent, SceneCursorPosition, SceneMouseClick,
    },
    simulation::CursorPosition,
    ui::sync::UiSyncWithCursor,
};

pub fn ui_mouse_hover_filterer(
    mut commands: Commands,
    cursor_pos: Res<CursorPosition>,
    nodes_query: Query<(Entity, &Node, &GlobalTransform, &ViewVisibility), Without<RayTransparent>>,
    mut scene_cursor_pos: ResMut<SceneCursorPosition>,
) {
    let Some(cursor_pos) = **cursor_pos else {
        return;
    };

    let mut blocked = false;

    for (entity, node, transform, vis) in &nodes_query {
        if vis.get() && node.logical_rect(transform).contains(cursor_pos) {
            blocked = true;
            commands.entity(entity).insert(MouseHovering);
        }
    }

    if !blocked {
        **scene_cursor_pos = Some(cursor_pos);
    }
}

pub fn ui_mouse_input_filterer(
    mut commands: Commands,
    cursor_pos: Res<CursorPosition>,
    nodes_query: Query<Entity, (With<MouseHovering>, With<Node>)>,
    mut mouse: EventReader<MouseButtonInput>,
    mut event: EventWriter<SceneMouseClick>,
) {
    let Some(cursor_pos) = **cursor_pos else {
        return;
    };

    for ev in mouse.read() {
        if nodes_query.is_empty() {
            event.send(SceneMouseClick {
                cursor_pos,
                button: ev.button,
                state: ev.state,
            });
        } else {
            nodes_query.iter().for_each(|entity| {
                commands.entity(entity).insert(MouseInput {
                    button: ev.button,
                    state: ev.state,
                });
            });
        }
    }
}

pub fn ui_drag_marker(
    commands: ParallelCommands,
    nodes_query: Query<(Entity, &MouseInput), With<Dragable>>,
    cursor_pos: Res<CursorPosition>,
) {
    let Some(cursor_pos) = **cursor_pos else {
        return;
    };

    nodes_query.par_iter().for_each(|(entity, input)| {
        if input.button != MouseButton::Left {
            return;
        }

        match input.state {
            ButtonState::Pressed => {
                commands.command_scope(|mut c| {
                    c.entity(entity).insert(UiSyncWithCursor {
                        initial_cursor_pos: cursor_pos,
                        ..Default::default()
                    });
                });
            }
            ButtonState::Released => {
                commands.command_scope(|mut c| {
                    c.entity(entity).remove::<UiSyncWithCursor>();
                });
            }
        }
    });
}

pub fn ui_drag_canceller(
    commands: ParallelCommands,
    mut nodes_query: Query<(Entity, &mut Style, &UiSyncWithCursor), (With<Dragable>, With<Node>)>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }

    nodes_query
        .par_iter_mut()
        .for_each(|(entity, mut style, on_drag)| {
            on_drag.initial_elem_pos.inspect(|pos| {
                pos.apply_to(&mut style);
            });

            commands.command_scope(|mut c| {
                c.entity(entity).remove::<UiSyncWithCursor>();
            });
        });
}
