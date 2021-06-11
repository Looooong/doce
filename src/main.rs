use bevy::{
    app::AppExit,
    pbr::AmbientLight,
    prelude::*,
    reflect::{TypeRegistry, TypeUuid},
    render::{
        camera::Camera,
        mesh::shape,
        pipeline::{PipelineDescriptor, RenderPipeline},
        render_graph::{base, AssetRenderResourcesNode, RenderGraph},
        renderer::RenderResources,
        shader::ShaderStages,
    },
};
use bevy_rapier3d::{physics::TimestepMode, prelude::*};

mod app_state;
mod cleanup;
mod debug;
mod main_menu;
mod physics;
mod resources;
mod ui;

use app_state::{AppState, InitAppStatePlugin};
use cleanup::{CleanupConfig, CleanupPlugin};
use debug::DebugPlugin;
use debug::DebugRigidBodyIndex;
use debug::DebugSimulationStateEvent;
use main_menu::MainMenuPlugin;
use physics::PhysicsPlugin;
use resources::CheckerboardMaterial;
use resources::GameReplay;
use resources::InitResourcesPlugin;
use resources::MainCharacterInput;
use resources::ShaderResources;
use resources::Tick;
use resources::UIResources;
use ui::UIPlugin;

fn main() {
    App::build()
        .add_plugins(DefaultPlugins)
        .add_plugin(PhysicsPlugin::<NoUserData>::default())
        // .add_plugin(bevy::diagnostic::LogDiagnosticsPlugin::default())
        // .add_plugin(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
        .add_asset::<CheckerboardMaterial>()
        .add_plugin(InitAppStatePlugin(AppState::MainMenu))
        .add_plugin(InitResourcesPlugin)
        .add_plugin(CleanupPlugin)
        .add_plugin(DebugPlugin)
        .add_plugin(MainMenuPlugin)
        .add_plugin(UIPlugin)
        .add_startup_system(setup.system())
        // Update
        // In-game
        .add_system_set(
            SystemSet::on_enter(AppState::InGame)
                .with_system(game_setup.system())
                .with_system(game_setup_replay.system())
                .with_system(game_setup_ui.system())
                .with_system(game_setup_main_character.system())
                .with_system(game_setup_environment.system()),
        )
        .add_system_set(
            SystemSet::on_update(AppState::InGame)
                .with_system(game_main_character_input.system().label("character_input"))
                .with_system(
                    game_main_character_input_record
                        .system()
                        .after("character_input"),
                )
                .with_system(game_ui.system().after("character_input"))
                .with_system(
                    game_main_character_movement
                        .system()
                        .after("character_input"),
                )
                .with_system(game_camera_movement.system())
                .with_system(game_save.exclusive_system()),
        )
        .add_system_set(SystemSet::on_exit(AppState::InGame).with_system(game_cleanup.system()))
        // Replay
        .add_system_set(
            SystemSet::on_enter(AppState::Replay)
                .with_system(replay_setup.system())
                .with_system(game_setup.system())
                .with_system(game_setup_ui.system())
                .with_system(game_setup_main_character.system())
                .with_system(game_setup_environment.system()),
        )
        .add_system_set(
            SystemSet::on_update(AppState::Replay)
                .with_system(
                    game_main_character_input_replay
                        .system()
                        .label("character_input"),
                )
                .with_system(game_ui.system().after("character_input"))
                .with_system(
                    game_main_character_movement
                        .system()
                        .after("character_input"),
                )
                .with_system(game_camera_movement.system()),
        )
        // Last
        .add_system_to_stage(CoreStage::Last, game_increment_tick.system())
        .run();
}

struct InitialEnvironment {
    boundaries: Vec<Boundary>,
    ball_template: BallTemplate,
}

struct BallTemplate {
    mesh: Handle<Mesh>,
    mesh_material: Handle<StandardMaterial>,
    rigid_body_positions: Vec<RigidBodyPosition>,
    rigid_body_damping: RigidBodyDamping,
    rigid_body_forces: RigidBodyForces,
    collider_shape: SharedShape,
    collider_material: ColliderMaterial,
}

struct Boundary {
    mesh: Handle<Mesh>,
    mesh_transform: Transform,
    collider_shape: SharedShape,
    collider_position: ColliderPosition,
}

struct TickText;

fn setup(
    mut commands: Commands,
    mut rapier_config: ResMut<RapierConfiguration>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    asset_server.watch_for_changes().unwrap();

    rapier_config.timestep_mode = TimestepMode::FixedTimestep;

    let transforms = vec![
        // Floor
        Transform::identity(),
        // Ceiling
        Transform::from_xyz(0.0, 20.0, 0.0).looking_at(Vec3::new(0.0, 20.0, 1.0), -Vec3::Y),
        // Wall X-
        Transform::from_xyz(-10.0, 10.0, 0.0).looking_at(Vec3::new(-10.0, 20.0, 0.0), Vec3::X),
        // Wall X+
        Transform::from_xyz(10.0, 10.0, 0.0).looking_at(Vec3::new(10.0, 20.0, 0.0), -Vec3::X),
        // Wall Z-
        Transform::from_xyz(0.0, 10.0, -10.0).looking_at(Vec3::new(0.0, 20.0, -10.0), Vec3::Z),
        // Wall Z+
        Transform::from_xyz(0.0, 10.0, 10.0).looking_at(Vec3::new(0.0, 20.0, 10.0), -Vec3::Z),
    ];

    let plane = meshes.add(Mesh::from(shape::Plane { size: 20.0 }));

    let boundaries = transforms
        .into_iter()
        .map(|transform| Boundary {
            mesh: plane.clone(),
            mesh_transform: transform,
            collider_shape: ColliderShape::halfspace(UnitVector::new_unchecked(
                transform.local_y().into(),
            )),
            collider_position: transform.translation.into(),
        })
        .collect();

    let mut rigid_body_positions = Vec::new();

    for i in -9..=9 {
        for j in -9..=9 {
            for k in 0..2 {
                rigid_body_positions.push(vector![i as f32, 4.0 + k as f32, j as f32].into());
            }
        }
    }

    commands.insert_resource(InitialEnvironment {
        boundaries,
        ball_template: BallTemplate {
            mesh: meshes.add(Mesh::from(shape::Icosphere {
                radius: 0.5,
                ..Default::default()
            })),
            mesh_material: materials.add(Color::CYAN.into()),
            rigid_body_damping: RigidBodyDamping {
                linear_damping: 0.8,
                angular_damping: 0.8,
            },
            rigid_body_forces: RigidBodyForces {
                gravity_scale: 0.2,
                ..Default::default()
            },
            rigid_body_positions,
            collider_shape: ColliderShape::ball(0.5),
            collider_material: ColliderMaterial {
                friction: 0.8,
                restitution: 0.8,
                ..Default::default()
            },
        },
    });
}

fn game_setup(
    mut commands: Commands,
    mut tick: ResMut<Tick>,
    mut rapier_config: ResMut<RapierConfiguration>,
) {
    tick.0 = 0;

    rapier_config.physics_pipeline_active = true;

    commands.spawn_bundle(LightBundle {
        light: Light {
            intensity: 500_000.0,
            range: 2_000.0,
            ..Default::default()
        },
        transform: Transform::from_xyz(0.0, 200.0, 400.0),
        ..Default::default()
    });
}

fn game_setup_replay(mut game_replay: ResMut<GameReplay>) {
    game_replay.main_character_inputs.clear();
}

fn game_setup_ui(mut commands: Commands, ui_resources: Res<UIResources>) {
    let button_bundle = ButtonBundle {
        style: Style {
            margin: Rect::all(Val::Px(5.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..Default::default()
        },
        ..Default::default()
    };

    commands.spawn_bundle(UiCameraBundle::default());

    commands
        .spawn_bundle(NodeBundle {
            style: Style {
                size: Size::new(Val::Percent(100.0), Val::Percent(100.0)),
                flex_direction: FlexDirection::ColumnReverse,
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::FlexStart,
                ..Default::default()
            },
            material: ui_resources.transparent.clone(),
            ..Default::default()
        })
        .with_children(|parent| {
            parent.spawn_bundle(TextBundle {
                style: Style {
                    margin: Rect::all(Val::Px(5.0)),
                    ..Default::default()
                },
                text: Text::with_section(
                    "Controls: WASD",
                    TextStyle {
                        font: ui_resources.font.clone(),
                        font_size: 24.0,
                        color: Color::GREEN,
                    },
                    Default::default(),
                ),
                ..Default::default()
            });

            parent
                .spawn_bundle(TextBundle {
                    style: Style {
                        margin: Rect::all(Val::Px(5.0)),
                        ..Default::default()
                    },
                    text: Text {
                        sections: vec![
                            TextSection {
                                value: String::from("Tick: "),
                                style: TextStyle {
                                    font: ui_resources.font.clone(),
                                    font_size: 24.0,
                                    color: Color::GREEN,
                                },
                                ..Default::default()
                            },
                            TextSection {
                                value: String::new(),
                                style: TextStyle {
                                    font: ui_resources.font.clone(),
                                    font_size: 24.0,
                                    color: Color::GREEN,
                                },
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .insert(TickText);

            parent
                .spawn_bundle(button_bundle.clone())
                .insert(ButtonType::RestartGame)
                .with_children(|parent| {
                    parent.spawn_bundle(TextBundle {
                        style: Style {
                            margin: Rect::all(Val::Px(5.0)),
                            ..Default::default()
                        },
                        text: Text::with_section(
                            "Restart Game",
                            TextStyle {
                                font: ui_resources.font.clone(),
                                font_size: 24.0,
                                color: Color::BLACK,
                            },
                            Default::default(),
                        ),
                        ..Default::default()
                    });
                });

            parent
                .spawn_bundle(button_bundle.clone())
                .insert(ButtonType::Replay)
                .with_children(|parent| {
                    parent.spawn_bundle(TextBundle {
                        style: Style {
                            margin: Rect::all(Val::Px(5.0)),
                            ..Default::default()
                        },
                        text: Text::with_section(
                            "Replay",
                            TextStyle {
                                font: ui_resources.font.clone(),
                                font_size: 24.0,
                                color: Color::BLACK,
                            },
                            Default::default(),
                        ),
                        ..Default::default()
                    });
                });
        });
}

fn game_setup_main_character(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let control_camera = commands
        .spawn_bundle(PerspectiveCameraBundle {
            transform: Transform::from_xyz(5.0, 5.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..Default::default()
        })
        .id();
    let rigid_body_position = vector![0.0, 1.0, 0.0].into();

    commands
        .spawn()
        .insert(MainCharacter {
            control_camera,
            control_camera_offset: Vec3::new(5.0, 5.0, 5.0),
        })
        .insert(MainCharacterMovement {
            want_to_move: Vec2::ZERO,
            walk_speed: 5.0,
            run_speed: 10.0,
        })
        .insert_bundle(PbrBundle {
            mesh: meshes.add(Mesh::from(shape::Capsule {
                depth: 1.0,
                radius: 0.5,
                ..Default::default()
            })),
            material: materials.add(StandardMaterial::default()),
            ..Default::default()
        })
        .insert_bundle(RigidBodyBundle {
            body_type: RigidBodyType::KinematicVelocityBased,
            position: rigid_body_position,
            ..Default::default()
        })
        .insert_bundle(ColliderBundle {
            shape: ColliderShape::capsule(point![0.0, -0.5, 0.0], point![0.0, 0.5, 0.0], 0.5),
            ..Default::default()
        })
        .insert(RigidBodyPositionSync::Discrete);
}

fn game_setup_environment(
    mut commands: Commands,
    initial_environment: Res<InitialEnvironment>,
    shader_resources: Res<ShaderResources>,
) {
    for boundary in &initial_environment.boundaries {
        commands
            .spawn_bundle(MeshBundle {
                mesh: boundary.mesh.clone(),
                render_pipelines: shader_resources.checkerboard_render_pipelines.clone(),
                transform: boundary.mesh_transform,
                ..Default::default()
            })
            .insert(shader_resources.checkerboard_material.clone())
            .insert_bundle(ColliderBundle {
                shape: boundary.collider_shape.clone(),
                position: boundary.collider_position,
                ..Default::default()
            });
    }

    let mut count = 0;

    for &rigid_body_position in &initial_environment.ball_template.rigid_body_positions {
        commands
            .spawn_bundle(PbrBundle {
                mesh: initial_environment.ball_template.mesh.clone(),
                material: initial_environment.ball_template.mesh_material.clone(),
                ..Default::default()
            })
            .insert_bundle(RigidBodyBundle {
                body_type: RigidBodyType::Dynamic,
                position: rigid_body_position,
                damping: initial_environment.ball_template.rigid_body_damping,
                forces: initial_environment.ball_template.rigid_body_forces,
                ..Default::default()
            })
            .insert_bundle(ColliderBundle {
                shape: initial_environment.ball_template.collider_shape.clone(),
                material: initial_environment.ball_template.collider_material,
                ..Default::default()
            })
            .insert(RigidBodyPositionSync::Discrete)
            .insert(DebugRigidBodyIndex(count));

        count += 1;
    }
}

fn game_ui(
    mut app_state: ResMut<State<AppState>>,
    mut cleanup_config: ResMut<CleanupConfig>,
    query: Query<(&Interaction, &ButtonType), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, button_type) in query.iter() {
        match interaction {
            Interaction::Clicked => {
                match button_type {
                    ButtonType::RestartGame => {
                        cleanup_config.next_state_after_cleanup = Some(AppState::InGame);
                        app_state.set(AppState::Cleanup).unwrap();
                    }
                    ButtonType::Replay => {
                        cleanup_config.next_state_after_cleanup = Some(AppState::Replay);
                        app_state.set(AppState::Cleanup).unwrap();
                    }
                    _ => (),
                };
            }
            _ => (),
        };
    }
}

fn game_main_character_input(
    input: Res<Input<KeyCode>>,
    mut query: Query<&mut MainCharacterMovement>,
) {
    for mut movement in query.iter_mut() {
        let mut want_to_move = Vec2::ZERO;

        if input.pressed(KeyCode::W) {
            want_to_move.y -= 1.0;
        }

        if input.pressed(KeyCode::A) {
            want_to_move.x -= 1.0;
        }

        if input.pressed(KeyCode::S) {
            want_to_move.y += 1.0;
        }

        if input.pressed(KeyCode::D) {
            want_to_move.x += 1.0;
        }

        let speed = if input.pressed(KeyCode::LShift) {
            movement.run_speed
        } else {
            movement.walk_speed
        };

        want_to_move = speed * want_to_move.normalize_or_zero();

        if movement.want_to_move != want_to_move {
            movement.want_to_move = want_to_move;
        }
    }
}

fn game_main_character_input_record(
    tick: Res<Tick>,
    mut game_replay: ResMut<GameReplay>,
    query: Query<&MainCharacterMovement, Changed<MainCharacterMovement>>,
) {
    game_replay.tick = *tick;

    for movement in query.iter() {
        info!(
            "Tick {} recorded movement {}",
            tick.0, movement.want_to_move
        );
        game_replay.main_character_inputs.push(MainCharacterInput {
            tick: *tick,
            movement: movement.want_to_move,
        });
    }
}

fn game_main_character_input_replay(
    tick: Res<Tick>,
    mut debug_events: EventWriter<DebugSimulationStateEvent>,
    mut game_replay: ResMut<GameReplay>,
    mut rapier_config: ResMut<RapierConfiguration>,
    mut query: Query<&mut MainCharacterMovement>,
) {
    if tick.0 >= game_replay.tick.0 {
        if rapier_config.physics_pipeline_active {
            debug_events.send(DebugSimulationStateEvent::Compare);
        }

        rapier_config.physics_pipeline_active = false;
        return;
    }

    loop {
        if game_replay.main_character_inputs_index >= game_replay.main_character_inputs.len() {
            return;
        }

        let input = &game_replay.main_character_inputs[game_replay.main_character_inputs_index];

        if input.tick.0 == tick.0 {
            let mut movement = query.single_mut().unwrap();
            movement.want_to_move = input.movement;
            game_replay.main_character_inputs_index += 1;
            return;
        } else if input.tick.0 > tick.0 {
            return;
        }

        game_replay.main_character_inputs_index += 1;
    }
}

fn game_main_character_movement(
    camera_query: Query<&Transform, With<Camera>>,
    mut character_query: Query<(
        &MainCharacter,
        &MainCharacterMovement,
        &mut RigidBodyPosition,
        &mut RigidBodyVelocity,
    )>,
) {
    for (character, character_movement, mut body_position, mut body_velocity) in
        character_query.iter_mut()
    {
        let camera_transform = camera_query.get(character.control_camera).unwrap();

        let mut velocity = character_movement.want_to_move.x * camera_transform.local_x();

        let mut forward = camera_transform.local_z();
        forward.y = 0.0;
        forward = forward.try_normalize().unwrap_or_else(|| {
            let mut forward = camera_transform.local_y();
            forward.y = 0.0;
            forward.try_normalize().unwrap() // Should not be zero at this point
        });

        velocity += character_movement.want_to_move.y * forward;

        body_velocity.linvel = velocity.into();

        body_position.position.translation = Vec3::clamp(
            body_position.position.translation.into(),
            Vec3::new(-9.5, 1.0, -9.5),
            Vec3::new(9.5, 19.0, 9.5),
        )
        .into();
    }
}

fn game_camera_movement(
    mut query_set: QuerySet<(
        Query<(&MainCharacter, &Transform)>,
        Query<&mut Transform, With<Camera>>,
    )>,
) {
    let (&character, &character_transform) = query_set.q0().single().unwrap();
    let mut camera_transform = query_set
        .q1_mut()
        .get_mut(character.control_camera)
        .unwrap();
    camera_transform.translation = Vec3::lerp(
        camera_transform.translation,
        character_transform.translation + character.control_camera_offset,
        0.02,
    );
}

fn game_save(world: &mut World) {
    let type_registry = world.get_resource::<TypeRegistry>().unwrap();
    let input = world.get_resource::<Input<KeyCode>>().unwrap();

    if input.pressed(KeyCode::LControl) && input.just_pressed(KeyCode::S) {
        let scene = DynamicScene::from_world(world, &type_registry);
        info!("{}", scene.serialize_ron(&type_registry).unwrap());
    }
}

fn game_cleanup(mut debug_events: EventWriter<DebugSimulationStateEvent>) {
    debug_events.send(DebugSimulationStateEvent::Record);
}

fn game_increment_tick(
    rapier_config: Res<RapierConfiguration>,
    mut tick: ResMut<Tick>,
    mut query: Query<&mut Text, With<TickText>>,
) {
    if !rapier_config.physics_pipeline_active {
        return;
    }

    tick.0 += 1;

    for mut text in query.iter_mut() {
        text.sections[1].value = format!("{}", tick.0);
    }
}

fn replay_setup(mut tick: ResMut<Tick>, mut game_replay: ResMut<GameReplay>) {
    tick.0 = 0;

    game_replay.main_character_inputs_index = 0;
}

#[derive(Clone, Copy)]
struct MainCharacter {
    control_camera: Entity,
    control_camera_offset: Vec3,
}

struct MainCharacterMovement {
    want_to_move: Vec2,
    walk_speed: f32,
    run_speed: f32,
}

enum ButtonType {
    Play,
    Quit,
    RestartGame,
    Replay,
}
