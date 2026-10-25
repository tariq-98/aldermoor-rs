//! Query states kept from step to step. Building one walks every archetype in the world, which
//! costs more than the query itself for the small, frequent ones here (every step, every death).

use bevy_ecs::prelude::*;
use bevy_ecs::query::{QueryData, QueryFilter};

use crate::{Auras, Control, Fighter, Motion, PlayerControl, Summoned, Threat, Unit, Vitals};
use crate::Brain;

pub(crate) type ControlQuery = QueryState<(Entity, Option<&'static Vitals>, Option<&'static Motion>, Option<&'static Auras>, &'static mut Control)>;
pub(crate) type UpkeepQuery =
    QueryState<(Entity, &'static Unit, &'static mut Vitals, Option<&'static mut Fighter>, Option<&'static Motion>, Option<&'static Brain>)>;
pub(crate) type ReviverQuery = QueryState<(Entity, &'static Unit, &'static Fighter)>;
pub(crate) type SummonQuery = QueryState<(Entity, &'static Unit, &'static Summoned, &'static Vitals)>;
pub(crate) type LetGoQuery = QueryState<(&'static mut Fighter, Option<&'static mut Threat>, Has<PlayerControl>)>;
pub(crate) type RallyQuery = QueryState<(Entity, &'static Unit, &'static Vitals, &'static Fighter), With<PlayerControl>>;

#[derive(Resource, Default)]
pub(crate) struct Queries {
    pub control: Option<ControlQuery>,
    pub upkeep: Option<UpkeepQuery>,
    pub revivers: Option<ReviverQuery>,
    pub summons: Option<SummonQuery>,
    pub let_go: Option<LetGoQuery>,
    pub rally: Option<RallyQuery>,
}

/// Runs `f` with the query state in `slot`, building it the first time. A nested call for the
/// same slot (a death inside a death) builds a fresh one rather than sharing.
pub(crate) fn with<D: QueryData + 'static, F: QueryFilter + 'static, R>(
    world: &mut World,
    slot: fn(&mut Queries) -> &mut Option<QueryState<D, F>>,
    f: impl FnOnce(&mut World, &mut QueryState<D, F>) -> R,
) -> R {
    let mut q = slot(&mut world.resource_mut::<Queries>()).take().unwrap_or_else(|| world.query_filtered::<D, F>());
    let r = f(world, &mut q);
    *slot(&mut world.resource_mut::<Queries>()) = Some(q);
    r
}

