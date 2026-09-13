//! Player state and movement (Stage 1: spawn + camera only).

use crate::level::Level;
use crate::map::{point_on_side, Map};
use crate::render::Camera;

/// Eye height above the floor.
pub const EYE: f32 = 41.0;

/// Player pose.
#[derive(Clone, Copy, Debug)]
pub struct Player {
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub sector: usize,
}

/// Finds the player-1 start and builds the initial pose.
pub fn spawn(level: &Level) -> Option<Player> {
    let t = level.map.things().iter().find(|t| t.kind == 1)?;
    let sector = sector_at(&level.map, t.x, t.y)?;
    Some(Player {
        x: t.x,
        y: t.y,
        angle: t.angle_deg as f32 * core::f32::consts::PI / 180.0,
        sector,
    })
}

/// Camera for the current player pose.
pub fn camera(level: &Level<'_>, player: &Player) -> Camera {
    let floor = level.map.sectors()[player.sector].floor_h;
    Camera {
        x: player.x,
        y: player.y,
        z: floor + EYE,
        angle: player.angle,
    }
}

/// Sector containing a point: BSP walk to a leaf, then seg -> side sector.
pub fn sector_at(map: &Map, x: f32, y: f32) -> Option<usize> {
    if map.node_count == 0 {
        return None;
    }
    let mut child = map.node_count - 1;
    loop {
        let node = &map.nodes[child];
        let side = point_on_side(x, y, node);
        let next = node.child[side] as usize;
        if next & 0x8000 != 0 {
            let sub = &map.subsectors[(next & 0x7fff) as usize % map.subsector_count.max(1)];
            if sub.seg_count == 0 {
                return None;
            }
            // First *valid* seg: index 0 can be a culled mini-seg.
            let nl = map.line_count.max(1);
            let ns = map.side_count.max(1);
            let nsec = map.sector_count.max(1);
            for k in 0..sub.seg_count as usize {
                let seg = &map.segs[sub.first_seg as usize + k];
                if seg.linedef == u16::MAX || (seg.linedef as usize) >= nl {
                    continue;
                }
                let line = &map.lines[seg.linedef as usize % nl];
                let side_idx = if seg.side == 0 {
                    line.front
                } else {
                    line.back
                };
                if side_idx == u16::MAX {
                    continue;
                }
                let side = &map.sides[side_idx as usize % ns];
                return Some(side.sector as usize % nsec);
            }
            return None;
        }
        child = next;
        if child >= map.node_count {
            return None;
        }
    }
}
