use anyhow::Context;
use dungers::varint;
use prost;
use prost::Message;
use std::io::{self, Read, Seek, SeekFrom};
use valveprotos::common::{
    CDemoClassInfo, CDemoFileInfo, CDemoFullPacket, CDemoPacket, CDemoSendTables, EDemoCommands,
};

use crate::demostream::{CmdHeader, DecodeCmdError, DemoStream, ReadCmdError, ReadCmdHeaderError};

// #define DEMO_RECORD_BUFFER_SIZE 2*1024*1024
//
// NOTE: read_cmd reads bytes (cmd_header.body_size) from the rdr into buf, if cmd is compressed
// (cmd_header.body_compressed) it'll decompress the data. buf must be large enough to fit
// compressed and uncompressed data simultaneously.
pub const DEMO_RECORD_BUFFER_SIZE: usize = 2 * 1024 * 1024;

// #define DEMO_HEADER_ID "HL2DEMO"
//
// NOTE: strings in c/cpp are null terminated.
const DEMO_HEADER_ID_SIZE: usize = 8;
const DEMO_HEADER_ID: [u8; DEMO_HEADER_ID_SIZE] = *b"PBDEMS2\0";

// NOTE: naming is based on stuff from demofile.h of valve's demoinfo2 thing.
#[derive(Debug, Clone)]
pub struct DemoHeader {
    pub demofilestamp: [u8; DEMO_HEADER_ID_SIZE],
    pub fileinfo_offset: i32,
    pub spawngroups_offset: i32,
}

#[derive(thiserror::Error, Debug)]
pub enum DemoHeaderError {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error("invalid demo file stamp (got {got:?}; want id {DEMO_HEADER_ID:?})")]
    InvalidDemoFileStamp { got: [u8; DEMO_HEADER_ID_SIZE] },
}

fn read_demo_header<R: Read>(mut rdr: R) -> Result<DemoHeader, DemoHeaderError> {
    let mut demofilestamp = [0u8; DEMO_HEADER_ID_SIZE];
    rdr.read_exact(&mut demofilestamp)?;
    if demofilestamp != DEMO_HEADER_ID {
        return Err(DemoHeaderError::InvalidDemoFileStamp { got: demofilestamp });
    }

    let mut buf = [0u8; size_of::<i32>()];

    rdr.read_exact(&mut buf)?;
    let fileinfo_offset = i32::from_le_bytes(buf);

    rdr.read_exact(&mut buf)?;
    let spawngroups_offset = i32::from_le_bytes(buf);

    Ok(DemoHeader {
        demofilestamp,
        fileinfo_offset,
        spawngroups_offset,
    })
}

#[derive(Debug)]
pub struct DemoFile<R: Read + Seek> {
    rdr: R,
    buf: Vec<u8>,
    demo_header: DemoHeader,
    file_info: Option<CDemoFileInfo>,
}

impl<R: Read + Seek> DemoFile<R> {
    /// creates a new [`DemoFile`] instance from the given reader.
    ///
    /// # performance note
    ///
    /// for optimal performance make sure to provide a reader that implements buffering (for
    /// example [`std::io::BufReader`]).
    pub fn start_reading(mut rdr: R) -> Result<Self, DemoHeaderError> {
        let demo_header = read_demo_header(&mut rdr)?;
        Ok(Self {
            rdr,
            buf: vec![0u8; DEMO_RECORD_BUFFER_SIZE],
            demo_header,
            file_info: None,
        })
    }

    pub fn demo_header(&self) -> &DemoHeader {
        &self.demo_header
    }

    pub fn file_info(&mut self) -> Result<&CDemoFileInfo, anyhow::Error> {
        if self.file_info.is_none() {
            let backup = self.stream_position()?;

            self.seek(SeekFrom::Start(self.demo_header.fileinfo_offset as u64))?;
            let cmd_header = self.read_cmd_header()?;
            self.file_info = Some(CDemoFileInfo::decode(self.read_cmd(&cmd_header)?)?);

            self.seek(SeekFrom::Start(backup))?;
        }

        self.file_info.as_ref().context("file info not set")
    }
}

impl<R: Read + Seek> DemoStream for DemoFile<R> {
    // stream ops
    // ----

    /// delegated from [`std::io::Seek`].
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, io::Error> {
        self.rdr.seek(pos)
    }

    /// delegated from [`std::io::Seek`].
    ///
    /// # note
    ///
    /// be aware that this method can be quite expensive. it might be best to make sure not to call
    /// it too frequently.
    fn stream_position(&mut self) -> Result<u64, io::Error> {
        self.rdr.stream_position()
    }

    // cmd header
    // ----

    fn read_cmd_header(&mut self) -> Result<CmdHeader, ReadCmdHeaderError> {
        const DEM_IS_COMPRESSED: u32 = EDemoCommands::DemIsCompressed as u32;
        let (cmd, cmd_n, body_compressed) = {
            let (cmd_raw, n) = varint::read_uvarint32(&mut self.rdr)?;

            let body_compressed = cmd_raw & DEM_IS_COMPRESSED == DEM_IS_COMPRESSED;

            let cmd = if body_compressed {
                cmd_raw & !DEM_IS_COMPRESSED
            } else {
                cmd_raw
            };

            (
                EDemoCommands::try_from(cmd as i32).map_err(|_| {
                    ReadCmdHeaderError::UnknownCmd {
                        raw: cmd_raw,
                        uncompressed: cmd,
                    }
                })?,
                n,
                body_compressed,
            )
        };

        let (tick, tick_n) = {
            let (tick, n) = varint::read_uvarint32(&mut self.rdr)?;
            // NOTE: tick is set to u32::MAX before before all pre-game initialization messages are
            // sent.
            // ticks everywhere are represented as i32, casting u32::MAX to i32 is okay because
            // bits in u32::MAX == bits in -1 i32.
            let tick = tick as i32;
            (tick, n)
        };

        let (body_size, body_size_n) = varint::read_uvarint32(&mut self.rdr)?;

        Ok(CmdHeader {
            cmd,
            body_compressed,
            tick,
            body_size,
            size: (cmd_n + tick_n + body_size_n) as u8,
        })
    }

    // cmd body
    // ----

    fn read_cmd(&mut self, cmd_header: &CmdHeader) -> Result<&[u8], ReadCmdError> {
        let (left, right) = self.buf.split_at_mut(cmd_header.body_size as usize);
        self.rdr.read_exact(left)?;

        if cmd_header.body_compressed {
            let decompress_len = snap::raw::decompress_len(left)?;
            snap::raw::Decoder::new().decompress(left, right)?;
            // NOTE: we need to slice stuff up, because prost's decode can't
            // determine when to stop.
            Ok(&right[..decompress_len])
        } else {
            Ok(left)
        }
    }

    fn decode_cmd_send_tables(data: &[u8]) -> Result<CDemoSendTables, DecodeCmdError> {
        CDemoSendTables::decode(data).map_err(DecodeCmdError::DecodeProtobufError)
    }

    fn decode_cmd_class_info(data: &[u8]) -> Result<CDemoClassInfo, DecodeCmdError> {
        CDemoClassInfo::decode(data).map_err(DecodeCmdError::DecodeProtobufError)
    }

    fn decode_cmd_packet(data: &[u8]) -> Result<CDemoPacket, DecodeCmdError> {
        CDemoPacket::decode(data).map_err(DecodeCmdError::DecodeProtobufError)
    }

    fn decode_cmd_full_packet(data: &[u8]) -> Result<CDemoFullPacket, DecodeCmdError> {
        CDemoFullPacket::decode(data).map_err(DecodeCmdError::DecodeProtobufError)
    }

    // other
    // ----

    fn start_position(&self) -> u64 {
        size_of::<DemoHeader>() as u64
    }

    fn total_ticks(&mut self) -> Result<i32, anyhow::Error> {
        self.file_info()
            .map(valveprotos::common::CDemoFileInfo::playback_ticks)
    }
}
