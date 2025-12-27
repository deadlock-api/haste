use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;
use std::fs::File;
use std::io::BufReader;

use anyhow::Context as _;
use haste::demofile::DemoFile;
use haste::entities::{
    DeltaHeader, Entity, GetValueError, deadlock_coord_from_cell, fkey_from_path,
};
use haste::fxhash;
use haste::parser::{Context, Parser, Visitor};

fn get_entity_coord(entity: &Entity, cell_key: &u64, vec_key: &u64) -> Result<f32, GetValueError> {
    let cell: u16 = entity.try_get_value(cell_key)?;
    let vec: f32 = entity.try_get_value(vec_key)?;
    let coord = deadlock_coord_from_cell(cell, vec);
    Ok(coord)
}

fn get_entity_position(entity: &Entity) -> Result<[f32; 3], GetValueError> {
    const CX: u64 = fkey_from_path(&[
        "CBodyComponent",
        "m_skeletonInstance",
        "m_vecOrigin",
        "m_cellX",
    ]);
    const CY: u64 = fkey_from_path(&[
        "CBodyComponent",
        "m_skeletonInstance",
        "m_vecOrigin",
        "m_cellY",
    ]);
    const CZ: u64 = fkey_from_path(&[
        "CBodyComponent",
        "m_skeletonInstance",
        "m_vecOrigin",
        "m_cellZ",
    ]);

    const VX: u64 = fkey_from_path(&[
        "CBodyComponent",
        "m_skeletonInstance",
        "m_vecOrigin",
        "m_vecX",
    ]);
    const VY: u64 = fkey_from_path(&[
        "CBodyComponent",
        "m_skeletonInstance",
        "m_vecOrigin",
        "m_vecY",
    ]);
    const VZ: u64 = fkey_from_path(&[
        "CBodyComponent",
        "m_skeletonInstance",
        "m_vecOrigin",
        "m_vecZ",
    ]);

    let x = get_entity_coord(entity, &CX, &VX)?;
    let y = get_entity_coord(entity, &CY, &VY)?;
    let z = get_entity_coord(entity, &CZ, &VZ)?;

    Ok([x, y, z])
}

const DEADLOCK_PLAYERPAWN_ENTITY: u64 = fxhash::hash_bytes(b"CCitadelPlayerPawn");

#[derive(Default, Debug)]
struct MyVisitor {
    positions: HashMap<i32, [f32; 3]>,
}

impl MyVisitor {
    fn handle_player_pawn(&mut self, entity: &Entity) -> Result<(), GetValueError> {
        let position = get_entity_position(entity)?;

        // TODO: get rid of hashmap, parser must supply a list of updated fields.
        match self.positions.entry(entity.index()) {
            Entry::Occupied(mut oe) => {
                let prev_position = oe.insert(position);
                if prev_position != position {
                    eprintln!(
                        "{} moved from {:?} to {:?}",
                        entity.index(),
                        prev_position,
                        position
                    );
                }
            }
            Entry::Vacant(ve) => {
                ve.insert(position);
            }
        };

        Ok(())
    }
}

#[derive(Debug)]
enum VisitError {
    GetValue(GetValueError),
}

impl std::error::Error for VisitError{}
impl fmt::Display for VisitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VisitError::GetValue(GetValueError::FieldNotExist) => write!(f, "field not exist"),
            VisitError::GetValue(GetValueError::FieldValueConversionError(_)) => write!(f, "conversion failed"),
        }
    }
}

impl Visitor for MyVisitor {
    type Error = VisitError;

    fn on_entity(
        &mut self,
        _ctx: &Context,
        _delta_header: DeltaHeader,
        entity: &Entity,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + Sync {
        async {
            if entity.serializer_name_heq(DEADLOCK_PLAYERPAWN_ENTITY) {
                self.handle_player_pawn(entity).map_err(|e| VisitError::GetValue(e))?;
            }

            Ok(())
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let filepath = args
        .get(1)
        .context("usage: deadlock-position <filepath>")
        .expect("proper usage");
    let file = File::open(filepath).expect("proper usage");
    let buf_reader = BufReader::new(file);
    let demo_file = DemoFile::start_reading(buf_reader).expect("can read");
    let mut parser = Parser::from_stream_with_visitor(demo_file, MyVisitor::default())
        .expect("can create parser");
    parser.run_to_end().await.expect("ran");
}
