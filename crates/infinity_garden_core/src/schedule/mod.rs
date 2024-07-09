use bevy::{
    app::{App, FixedUpdate, Plugin, Startup},
    prelude::IntoSystemSetConfigs,
    state::{
        app::AppExtStates,
        condition::in_state,
        state::{OnEnter, OnTransition},
    },
};

use crate::schedule::{
    state::{AssetState, GameState, ProcessState},
    system_set::{InitializeSet, PrepareSet, SimulationSet},
};

pub mod state;
pub mod system_set;

pub struct InfGdnSchedulePlugin;

impl Plugin for InfGdnSchedulePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<ProcessState>()
            .init_state::<AssetState>()
            .init_state::<GameState>()
            .configure_sets(
                Startup,
                PrepareSet
                    .run_if(in_state(ProcessState::Prepare))
                    .run_if(in_state(AssetState::Load)),
            )
            .configure_sets(
                OnEnter(GameState::Initialize),
                InitializeSet
                    .run_if(in_state(ProcessState::InGame))
                    .run_if(in_state(AssetState::Finish)),
            )
            .configure_sets(
                FixedUpdate,
                SimulationSet
                    .run_if(in_state(ProcessState::InGame))
                    .run_if(in_state(AssetState::Finish))
                    .run_if(in_state(GameState::Simulate)),
            );
    }
}
