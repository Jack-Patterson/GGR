//! The 3D presenter: a grey-box hall built from the same cells the sim walks on, primitives for
//! each prefab, and capsules for people (V2's standing policy: capsules until the art pass).
//! It draws walks the sim has already timed and decides nothing.

use std::collections::HashMap;

use bevy::prelude::*;
use ggr_content::PrefabKind;
use ggr_sim::{Activity, CharState, InstanceStatus, World, CELL_DOOR, CELL_LOCKED, CELL_WALL};

use crate::state::*;
use crate::theme;

const WALL_HEIGHT: f32 = 1.3;
const CAPSULE_RADIUS: f32 = 0.24;
const CAPSULE_LENGTH: f32 = 0.66;

#[derive(Component)]
pub struct HallPiece;

#[derive(Component)]
pub struct InstView {
    pub ready: bool,
}

#[derive(Component)]
pub struct CharView {
    pub id: u32,
}

#[derive(Component)]
pub struct Body;

#[derive(Component)]
pub struct CandidateView;

/// Shared meshes and a material cache.
#[derive(Resource)]
pub struct Assets3d {
    pub cube: Handle<Mesh>,
    pub capsule: Handle<Mesh>,
    pub cylinder: Handle<Mesh>,
    pub sphere: Handle<Mesh>,
    materials: HashMap<(u8, u8, u8, u8), Handle<StandardMaterial>>,
}

impl Assets3d {
    pub fn mat(
        &mut self,
        mats: &mut Assets<StandardMaterial>,
        rgb: [u8; 3],
        alpha: u8,
    ) -> Handle<StandardMaterial> {
        let key = (rgb[0], rgb[1], rgb[2], alpha);
        self.materials
            .entry(key)
            .or_insert_with(|| {
                mats.add(StandardMaterial {
                    base_color: Color::srgba_u8(rgb[0], rgb[1], rgb[2], alpha),
                    perceptual_roughness: 0.92,
                    alpha_mode: if alpha < 255 {
                        AlphaMode::Blend
                    } else {
                        AlphaMode::Opaque
                    },
                    ..default()
                })
            })
            .clone()
    }
}

/// A drawn walk: its departure minute, its destination, and the cells between.
type WalkPath = (i64, (i32, i32), Vec<Vec2>);

/// What the presenter has spawned, keyed by sim id, and the walk paths it is drawing.
#[derive(Resource, Default)]
pub struct ViewIndex {
    pub generation: Option<u32>,
    pub east_open: bool,
    pub instances: HashMap<u32, Entity>,
    pub chars: HashMap<u32, Entity>,
    pub candidates: HashMap<u32, Entity>,
    paths: HashMap<u32, WalkPath>,
}

pub fn cell_center(c: (i32, i32)) -> Vec3 {
    Vec3::new(c.0 as f32 + 0.5, 0.0, c.1 as f32 + 0.5)
}

pub fn setup_scene(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(Assets3d {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        capsule: meshes.add(Capsule3d::new(CAPSULE_RADIUS, CAPSULE_LENGTH)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
        sphere: meshes.add(Sphere::new(0.5)),
        materials: HashMap::new(),
    });
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(1.0, 0.97, 0.92),
        brightness: 450.0,
        affects_lightmapped_meshes: true,
    });
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: true,
            color: Color::srgb(1.0, 0.96, 0.9),
            ..default()
        },
        Transform::from_xyz(30.0, 40.0, 30.0).looking_at(Vec3::new(18.0, 0.0, 12.0), Vec3::Y),
    ));
}

/// Rebuilds floor and walls whenever the world is replaced or the East Wing opens.
pub fn sync_hall(
    mut commands: Commands,
    session: Res<Session>,
    mut index: ResMut<ViewIndex>,
    mut a: ResMut<Assets3d>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    pieces: Query<Entity, With<HallPiece>>,
    insts: Query<Entity, With<InstView>>,
    chars: Query<Entity, With<CharView>>,
    cands: Query<Entity, With<CandidateView>>,
) {
    let Some(world) = session.world.as_ref() else {
        return;
    };
    let gen_changed = index.generation != Some(session.generation);
    if !gen_changed && index.east_open == world.east_wing_open() {
        return;
    }
    if gen_changed {
        for e in insts.iter().chain(chars.iter()).chain(cands.iter()) {
            commands.entity(e).despawn();
        }
        index.instances.clear();
        index.chars.clear();
        index.candidates.clear();
        index.paths.clear();
    }
    for e in &pieces {
        commands.entity(e).despawn();
    }
    index.generation = Some(session.generation);
    index.east_open = world.east_wing_open();

    let g = world.grid();
    let floor = a.mat(&mut mats, [0xD8, 0xD0, 0xBF], 255);
    let locked_floor = a.mat(&mut mats, [0xB9, 0xB2, 0xA3], 255);
    let wall = a.mat(&mut mats, [0x8E, 0x86, 0x78], 255);
    let east_wall = a.mat(&mut mats, [0x7A, 0x73, 0x67], 255);
    let door = a.mat(&mut mats, [0x5A, 0x3E, 0x2B], 255);
    let rug = a.mat(&mut mats, [0x8C, 0x2F, 0x26], 255);
    let region = world.content().layout.east_region;

    // Floor: the main hall as one slab, the East Wing as another (darker while closed).
    let (w, h) = (g.width as f32, g.height as f32);
    commands.spawn((
        Mesh3d(a.cube.clone()),
        MeshMaterial3d(floor),
        Transform::from_xyz(region.x as f32 / 2.0, -0.05, h / 2.0).with_scale(Vec3::new(
            region.x as f32,
            0.1,
            h,
        )),
        HallPiece,
    ));
    commands.spawn((
        Mesh3d(a.cube.clone()),
        MeshMaterial3d(if world.east_wing_open() {
            a.mat(&mut mats, [0xD8, 0xD0, 0xBF], 255)
        } else {
            locked_floor
        }),
        Transform::from_xyz((region.x as f32 + w) / 2.0, -0.05, h / 2.0).with_scale(Vec3::new(
            w - region.x as f32,
            0.1,
            h,
        )),
        HallPiece,
    ));
    // A runner from the door, so the entrance reads at a glance.
    let d = g.door;
    commands.spawn((
        Mesh3d(a.cube.clone()),
        MeshMaterial3d(rug),
        Transform::from_xyz(d.0 as f32 + 2.0, 0.005, d.1 as f32 + 0.5)
            .with_scale(Vec3::new(3.0, 0.01, 1.0)),
        HallPiece,
    ));
    let east_wall_rect = world.content().layout.east_wall;
    for y in 0..g.height {
        for x in 0..g.width {
            let f = g.flags(x, y);
            if f & CELL_WALL == 0 {
                continue;
            }
            // The door is a gap in the west wall.
            if x == d.0 - 1 && y == d.1 {
                commands.spawn((
                    Mesh3d(a.cube.clone()),
                    MeshMaterial3d(door.clone()),
                    Transform::from_xyz(x as f32 + 0.5, 0.9, y as f32 + 0.5)
                        .with_scale(Vec3::new(0.3, 1.8, 0.9)),
                    HallPiece,
                ));
                continue;
            }
            let is_east = east_wall_rect.contains(x, y);
            commands.spawn((
                Mesh3d(a.cube.clone()),
                MeshMaterial3d(if is_east {
                    east_wall.clone()
                } else {
                    wall.clone()
                }),
                Transform::from_xyz(x as f32 + 0.5, WALL_HEIGHT / 2.0, y as f32 + 0.5)
                    .with_scale(Vec3::new(1.0, WALL_HEIGHT, 1.0)),
                HallPiece,
            ));
        }
    }
    let _ = (CELL_DOOR, CELL_LOCKED);
}

fn prefab_color(kind: PrefabKind) -> [u8; 3] {
    match kind {
        PrefabKind::Desk => [0x6B, 0x4A, 0x32],
        PrefabKind::Board => [0x8A, 0x6B, 0x45],
        PrefabKind::Rest => [0x9C, 0x7B, 0x55],
        PrefabKind::Training => [0xA8, 0x8E, 0x5E],
        PrefabKind::Canteen => [0x7D, 0x5A, 0x3C],
        PrefabKind::Infirmary => [0xE6, 0xE1, 0xD3],
        PrefabKind::Decor => [0x8C, 0x2F, 0x26],
    }
}

/// Spawns one prefab's primitives as children of a root at its footprint's centre.
fn spawn_prefab(
    commands: &mut Commands,
    a: &mut Assets3d,
    mats: &mut Assets<StandardMaterial>,
    world: &World,
    id: u32,
) -> Entity {
    let inst = &world.instances()[id as usize];
    let p = &world.content().prefabs[inst.prefab as usize];
    let (mut minx, mut miny, mut maxx, mut maxy) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (dx, dy) in &p.footprint {
        minx = minx.min(inst.origin.0 + dx);
        maxx = maxx.max(inst.origin.0 + dx);
        miny = miny.min(inst.origin.1 + dy);
        maxy = maxy.max(inst.origin.1 + dy);
    }
    let fw = (maxx - minx + 1) as f32;
    let fd = (maxy - miny + 1) as f32;
    let center = Vec3::new(minx as f32 + fw / 2.0, 0.0, miny as f32 + fd / 2.0);
    let ready = inst.status == InstanceStatus::Ready;
    let root = commands
        .spawn((
            Transform::from_translation(center),
            Visibility::Visible,
            InstView { ready },
        ))
        .id();
    let ht = p.height;
    let base = prefab_color(p.kind);
    if !ready {
        // A construction site: a low translucent block and corner posts.
        let site = a.mat(mats, [0xB0, 0x8A, 0x3C], 150);
        let post = a.mat(mats, [0x6E, 0x6A, 0x5F], 255);
        commands.entity(root).with_children(|c| {
            c.spawn((
                Mesh3d(a.cube.clone()),
                MeshMaterial3d(site),
                Transform::from_xyz(0.0, 0.15, 0.0).with_scale(Vec3::new(
                    fw * 0.95,
                    0.3,
                    fd * 0.95,
                )),
            ));
            for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                c.spawn((
                    Mesh3d(a.cube.clone()),
                    MeshMaterial3d(post.clone()),
                    Transform::from_xyz(sx * fw * 0.45, 0.6, sz * fd * 0.45)
                        .with_scale(Vec3::new(0.08, 1.2, 0.08)),
                ));
            }
        });
        return root;
    }
    let m = a.mat(mats, base, 255);
    let dark = a.mat(mats, theme::INK, 255);
    let cream = a.mat(mats, theme::SURFACE, 255);
    let green = a.mat(mats, [0x4F, 0x7A, 0x52], 255);
    let accent = a.mat(mats, theme::ACCENT, 255);
    let stone = a.mat(mats, [0x9A, 0x95, 0x8A], 255);
    let (cube, cyl, sph) = (a.cube.clone(), a.cylinder.clone(), a.sphere.clone());
    commands.entity(root).with_children(|c| match p.kind {
        PrefabKind::Desk | PrefabKind::Canteen => {
            c.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(m.clone()),
                Transform::from_xyz(0.0, ht / 2.0, 0.0).with_scale(Vec3::new(
                    fw * 0.95,
                    ht,
                    fd * 0.8,
                )),
            ));
            c.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(cream.clone()),
                Transform::from_xyz(0.0, ht + 0.02, 0.0).with_scale(Vec3::new(
                    fw * 0.97,
                    0.04,
                    fd * 0.85,
                )),
            ));
            if p.kind == PrefabKind::Canteen {
                for i in 0..3 {
                    c.spawn((
                        Mesh3d(cyl.clone()),
                        MeshMaterial3d(dark.clone()),
                        Transform::from_xyz(-fw / 3.0 + i as f32 * fw / 3.0, ht + 0.1, 0.0)
                            .with_scale(Vec3::new(0.3, 0.16, 0.3)),
                    ));
                }
            }
        }
        PrefabKind::Board => {
            c.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(m.clone()),
                Transform::from_xyz(0.0, ht / 2.0, 0.0).with_scale(Vec3::new(fw * 0.9, ht, 0.15)),
            ));
            for i in 0..4 {
                let x = -fw * 0.3 + (i % 2) as f32 * fw * 0.35;
                let y = ht * 0.55 + (i / 2) as f32 * 0.45;
                c.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(cream.clone()),
                    Transform::from_xyz(x, y, 0.09).with_scale(Vec3::new(0.35, 0.3, 0.02)),
                ));
            }
        }
        PrefabKind::Rest => {
            if p.footprint.len() > 1 {
                c.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(m.clone()),
                    Transform::from_xyz(0.0, ht * 0.8, 0.0).with_scale(Vec3::new(
                        fw * 0.95,
                        0.12,
                        fd * 0.6,
                    )),
                ));
                for sx in [-1.0, 1.0] {
                    c.spawn((
                        Mesh3d(cube.clone()),
                        MeshMaterial3d(m.clone()),
                        Transform::from_xyz(sx * fw * 0.4, ht * 0.4, 0.0).with_scale(Vec3::new(
                            0.1,
                            ht * 0.8,
                            fd * 0.5,
                        )),
                    ));
                }
            } else {
                // The well: a stone ring and a little roof.
                c.spawn((
                    Mesh3d(cyl.clone()),
                    MeshMaterial3d(stone.clone()),
                    Transform::from_xyz(0.0, ht / 2.0, 0.0).with_scale(Vec3::new(0.9, ht, 0.9)),
                ));
                c.spawn((
                    Mesh3d(cyl.clone()),
                    MeshMaterial3d(dark.clone()),
                    Transform::from_xyz(0.0, ht + 0.01, 0.0).with_scale(Vec3::new(0.7, 0.02, 0.7)),
                ));
            }
        }
        PrefabKind::Training => {
            c.spawn((
                Mesh3d(cyl.clone()),
                MeshMaterial3d(m.clone()),
                Transform::from_xyz(0.0, ht * 0.4, 0.0).with_scale(Vec3::new(0.35, ht * 0.8, 0.35)),
            ));
            c.spawn((
                Mesh3d(sph.clone()),
                MeshMaterial3d(m.clone()),
                Transform::from_xyz(0.0, ht * 0.9, 0.0).with_scale(Vec3::splat(0.35)),
            ));
            c.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(m.clone()),
                Transform::from_xyz(0.0, ht * 0.65, 0.0).with_scale(Vec3::new(0.8, 0.1, 0.1)),
            ));
        }
        PrefabKind::Infirmary => {
            for i in 0..p.footprint.len() {
                let x = -fw / 2.0 + 0.5 + i as f32;
                c.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(m.clone()),
                    Transform::from_xyz(x, ht / 2.0, 0.0).with_scale(Vec3::new(0.8, ht, 0.9)),
                ));
                c.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(accent.clone()),
                    Transform::from_xyz(x, ht + 0.02, 0.2).with_scale(Vec3::new(0.82, 0.05, 0.45)),
                ));
            }
        }
        PrefabKind::Decor => {
            if ht > 1.5 {
                c.spawn((
                    Mesh3d(cyl.clone()),
                    MeshMaterial3d(dark.clone()),
                    Transform::from_xyz(0.0, ht / 2.0, 0.0).with_scale(Vec3::new(0.08, ht, 0.08)),
                ));
                c.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(accent.clone()),
                    Transform::from_xyz(0.0, ht * 0.7, 0.0).with_scale(Vec3::new(
                        0.6,
                        ht * 0.45,
                        0.04,
                    )),
                ));
            } else {
                c.spawn((
                    Mesh3d(cyl.clone()),
                    MeshMaterial3d(m.clone()),
                    Transform::from_xyz(0.0, ht * 0.3, 0.0).with_scale(Vec3::new(
                        0.6,
                        ht * 0.6,
                        0.6,
                    )),
                ));
                c.spawn((
                    Mesh3d(sph.clone()),
                    MeshMaterial3d(green.clone()),
                    Transform::from_xyz(0.0, ht * 0.75, 0.0).with_scale(Vec3::splat(0.7)),
                ));
            }
        }
    });
    root
}

pub fn sync_instances(
    mut commands: Commands,
    session: Res<Session>,
    mut index: ResMut<ViewIndex>,
    mut a: ResMut<Assets3d>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    views: Query<&InstView>,
) {
    let Some(world) = session.world.as_ref() else {
        return;
    };
    for inst in world.instances() {
        let ready = inst.status == InstanceStatus::Ready;
        let existing = index.instances.get(&inst.id).copied();
        match (existing, inst.status) {
            (Some(e), InstanceStatus::Demolished) => {
                commands.entity(e).despawn();
                index.instances.remove(&inst.id);
            }
            (None, InstanceStatus::Demolished) => {}
            (Some(e), _) => {
                if views.get(e).is_ok_and(|v| v.ready != ready) {
                    commands.entity(e).despawn();
                    let ne = spawn_prefab(&mut commands, &mut a, &mut mats, world, inst.id);
                    index.instances.insert(inst.id, ne);
                }
            }
            (None, _) => {
                let ne = spawn_prefab(&mut commands, &mut a, &mut mats, world, inst.id);
                index.instances.insert(inst.id, ne);
            }
        }
    }
}

/// Colour of a character's capsule: branch tint for adventurers, ink for staff, accent when
/// injured.
pub fn char_color(world: &World, id: u32) -> [u8; 3] {
    let c = &world.characters()[id as usize];
    if c.is_injured(world.minute()) {
        return theme::ACCENT;
    }
    match &c.adv {
        Some(adv) => {
            let branch = world.content().classes[adv.class as usize].branch;
            theme::BRANCH[branch % theme::BRANCH.len()]
        }
        None => [0x3A, 0x37, 0x30],
    }
}

fn point_along(path: &[Vec2], t: f32) -> (Vec2, Vec2) {
    if path.len() < 2 {
        let p = path.first().copied().unwrap_or(Vec2::ZERO);
        return (p, Vec2::Y);
    }
    let total: f32 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut d = total * t.clamp(0.0, 1.0);
    for w in path.windows(2) {
        let seg = w[0].distance(w[1]);
        if d <= seg || seg == 0.0 {
            let dir = (w[1] - w[0]).normalize_or_zero();
            return (w[0] + dir * d.min(seg), dir);
        }
        d -= seg;
    }
    let n = path.len();
    (path[n - 1], (path[n - 1] - path[n - 2]).normalize_or_zero())
}

#[allow(clippy::too_many_arguments)]
pub fn sync_characters(
    mut commands: Commands,
    session: Res<Session>,
    pace: Res<Pace>,
    mut index: ResMut<ViewIndex>,
    mut a: ResMut<Assets3d>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut roots: Query<(&mut Transform, &mut Visibility), (With<CharView>, Without<Body>)>,
    mut bodies: Query<
        (
            &ChildOf,
            &mut MeshMaterial3d<StandardMaterial>,
            &mut Transform,
        ),
        With<Body>,
    >,
) {
    let Some(world) = session.world.as_ref() else {
        return;
    };
    let now = world.minute() as f64
        + if session.mode == Mode::Playing {
            pace.frac
        } else {
            0.0
        };
    let door = world.grid().door;
    for c in world.characters() {
        let entity = *index.chars.entry(c.id).or_insert_with(|| {
            commands
                .spawn((
                    Transform::from_translation(cell_center(c.cell)),
                    Visibility::Hidden,
                    CharView { id: c.id },
                ))
                .with_children(|p| {
                    p.spawn((
                        Mesh3d(a.capsule.clone()),
                        MeshMaterial3d(Handle::<StandardMaterial>::default()),
                        Transform::from_xyz(0.0, CAPSULE_LENGTH / 2.0 + CAPSULE_RADIUS, 0.0),
                        Body,
                    ));
                    // A visor, so a capsule has a front.
                    p.spawn((
                        Mesh3d(a.cube.clone()),
                        MeshMaterial3d(a.mat(&mut mats, theme::INK, 255)),
                        Transform::from_xyz(0.0, CAPSULE_LENGTH + 0.12, CAPSULE_RADIUS * 0.85)
                            .with_scale(Vec3::new(0.26, 0.07, 0.08)),
                    ));
                })
                .id()
        });
        let Ok((mut tf, mut vis)) = roots.get_mut(entity) else {
            continue;
        };
        if !c.on_map() {
            *vis = Visibility::Hidden;
            index.paths.remove(&c.id);
            continue;
        }
        *vis = Visibility::Visible;
        let (pos, facing) = match (c.state, c.walk) {
            (CharState::Travel, Some(w)) => {
                let key = (w.depart, w.to);
                let fresh = index
                    .paths
                    .get(&c.id)
                    .is_some_and(|(d, to, _)| (*d, *to) == key);
                if !fresh {
                    let pts: Vec<Vec2> = world
                        .path(w.from, w.to)
                        .into_iter()
                        .map(|p| Vec2::new(p.0 as f32 + 0.5, p.1 as f32 + 0.5))
                        .collect();
                    index.paths.insert(c.id, (w.depart, w.to, pts));
                }
                let pts = &index.paths[&c.id].2;
                let span = (w.arrive - w.depart).max(1) as f64;
                let t = ((now - w.depart as f64) / span) as f32;
                let (p, dir) = if pts.is_empty() {
                    let a = Vec2::new(w.from.0 as f32 + 0.5, w.from.1 as f32 + 0.5);
                    let b = Vec2::new(w.to.0 as f32 + 0.5, w.to.1 as f32 + 0.5);
                    (a.lerp(b, t.clamp(0.0, 1.0)), (b - a).normalize_or_zero())
                } else {
                    point_along(pts, t)
                };
                (Vec3::new(p.x, 0.0, p.y), dir)
            }
            _ => {
                let mut p = cell_center(c.cell);
                if c.cell == door {
                    // Several people can stand in the doorway; fan them out.
                    let k = (c.id % 5) as f32 - 2.0;
                    p += Vec3::new(0.25 * (k % 2.0).abs(), 0.0, 0.18 * k);
                }
                // Face whatever they are using.
                let facing = match c.activity {
                    Activity::Using { inst, .. }
                    | Activity::Queueing { inst, .. }
                    | Activity::Working { inst } => {
                        let o = world.instances()[inst as usize].origin;
                        let fp = &world.content().prefabs
                            [world.instances()[inst as usize].prefab as usize]
                            .footprint;
                        let n = fp.len() as f32;
                        let cx = fp.iter().map(|f| f.0 as f32).sum::<f32>() / n + o.0 as f32 + 0.5;
                        let cy = fp.iter().map(|f| f.1 as f32).sum::<f32>() / n + o.1 as f32 + 0.5;
                        (Vec2::new(cx, cy) - Vec2::new(p.x, p.z)).normalize_or_zero()
                    }
                    Activity::None => Vec2::Y,
                };
                (p, facing)
            }
        };
        tf.translation = pos;
        if facing.length_squared() > 0.0 {
            tf.rotation = Quat::from_rotation_y(facing.x.atan2(facing.y));
        }
        // Lying down in the infirmary.
        let lying = matches!(c.activity, Activity::Using { inst, .. } if world.kind_of(inst) == PrefabKind::Infirmary);
        let color = char_color(world, c.id);
        let handle = a.mat(&mut mats, color, 255);
        for (parent, mut m, mut btf) in &mut bodies {
            if parent.parent() == entity {
                if m.0 != handle {
                    m.0 = handle.clone();
                }
                if lying {
                    btf.rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
                    btf.translation.y = 0.75;
                } else {
                    btf.rotation = Quat::IDENTITY;
                    btf.translation.y = CAPSULE_LENGTH / 2.0 + CAPSULE_RADIUS;
                }
            }
        }
    }
}

/// Candidates waiting at the desk, drawn as translucent capsules queued in front of it. Pure
/// presentation: candidates have no position in the sim.
pub fn sync_candidates(
    mut commands: Commands,
    session: Res<Session>,
    mut index: ResMut<ViewIndex>,
    mut a: ResMut<Assets3d>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Some(world) = session.world.as_ref() else {
        return;
    };
    let desk = world
        .instances()
        .iter()
        .find(|i| i.is_ready() && world.kind_of(i.id) == PrefabKind::Desk)
        .map(|i| i.origin);
    let waiting: Vec<u32> = world.waiting_candidates().map(|c| c.id).collect();
    index.candidates.retain(|id, e| {
        let keep = waiting.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    let Some(o) = desk else { return };
    for (i, id) in waiting.iter().enumerate() {
        let pos = Vec3::new(
            o.0 as f32 + 0.5 + (i % 2) as f32 * 0.8,
            CAPSULE_LENGTH / 2.0 + CAPSULE_RADIUS,
            o.1 as f32 + 1.6 + (i / 2) as f32 * 0.8,
        );
        if let Some(e) = index.candidates.get(id) {
            commands.entity(*e).insert(Transform::from_translation(pos));
            continue;
        }
        let staff = world
            .candidates()
            .iter()
            .find(|c| c.id == *id)
            .is_some_and(|c| c.staff_role.is_some());
        let m = a.mat(
            &mut mats,
            if staff {
                [0x3A, 0x37, 0x30]
            } else {
                [0x6E, 0x6A, 0x5F]
            },
            110,
        );
        let e = commands
            .spawn((
                Mesh3d(a.capsule.clone()),
                MeshMaterial3d(m),
                Transform::from_translation(pos),
                CandidateView,
            ))
            .id();
        index.candidates.insert(*id, e);
    }
}
