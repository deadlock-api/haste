use std::io::{self, SeekFrom};

use dungers::varint;
use valveprotos::common::{
    CDemoClassInfo, CDemoFullPacket, CDemoPacket, CDemoSendTables, EDemoCommands,
};

#[derive(Debug, Clone)]
pub struct CmdHeader {
    pub cmd: EDemoCommands,
    pub body_compressed: bool,
    pub tick: i32,
    pub body_size: u32,
    // NOTE: it is siginficantly cheaper to sum n bytes that were read (cmd, tick body_size) then
    // to rely on Seek::stream_position.
    //
    /// size of the cmd header (/ how many bytes were read). can be used to unread the cmd header.
    pub size: u8,
}

#[derive(thiserror::Error, Debug)]
pub enum ReadCmdHeaderError {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error(transparent)]
    ReadVarintError(#[from] varint::VarintError),
    #[error("unknown cmd (raw {raw}; uncompressed {uncompressed})")]
    UnknownCmd { raw: u32, uncompressed: u32 },
}

#[derive(thiserror::Error, Debug)]
pub enum ReadCmdError {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error(transparent)]
    DecompressError(#[from] snap::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum DecodeCmdError {
    #[error(transparent)]
    DecodeProtobufError(#[from] prost::DecodeError),
}

// TODO: is there a way to restrict (idk if this is a correct word) DemoStream trait so that it'll
// require seek ops to be implemented only if the underlying thing binds to io::Seek trait? thus
// making so that Parser will not provide methods (such as run_to_tick) that need seeking
// capabilities?

pub trait DemoStream {
    // stream ops
    // ----

    fn seek(&mut self, pos: SeekFrom) -> Result<u64, io::Error>;

    fn stream_position(&mut self) -> Result<u64, io::Error>;

    /// reimplementation of nightly [`std::io::Seek::stream_len`].
    fn stream_len(&mut self) -> Result<u64, io::Error> {
        let old_pos = self.stream_position()?;
        let len = self.seek(SeekFrom::End(0))?;

        // avoid seeking a third time when we were already at the end of the
        // stream. the branch is usually way cheaper than a seek operation.
        if old_pos != len {
            self.seek(SeekFrom::Start(old_pos))?;
        }

        Ok(len)
    }

    fn is_at_eof(&mut self) -> Result<bool, io::Error> {
        Ok(self.stream_position()? == self.stream_len()?)
    }

    // cmd header
    // ----

    fn read_cmd_header(&mut self) -> Result<CmdHeader, ReadCmdHeaderError>;

    fn unread_cmd_header(&mut self, cmd_header: &CmdHeader) -> Result<(), io::Error> {
        self.seek(SeekFrom::Current(-i64::from(cmd_header.size)))
            .map(|_| ())
    }

    // cmd
    // ----

    fn read_cmd(&mut self, cmd_header: &CmdHeader) -> Result<&[u8], ReadCmdError>;

    // TODO: should DemoStream require decoders for all cmds to be implemented?
    //
    // Error (no msg)
    // Stop (empty msg)
    // fn decode_cmd_file_header(data: &[u8]) -> Result<CDemoFileHeader, DecodeCmdError>;
    // fn decode_cmd_file_info(data: &[u8]) -> Result<CDemoFileInfo, DecodeCmdError>;
    // SyncTick (empty msg)
    fn decode_cmd_send_tables(data: &[u8]) -> Result<CDemoSendTables, DecodeCmdError>;
    fn decode_cmd_class_info(data: &[u8]) -> Result<CDemoClassInfo, DecodeCmdError>;
    // fn decode_cmd_string_tables(data: &[u8]) -> Result<CDemoStringTables, DecodeCmdError>;
    fn decode_cmd_packet(data: &[u8]) -> Result<CDemoPacket, DecodeCmdError>;
    // SignonPacket (same as Packet)
    // fn decode_cmd_console_cmd(data: &[u8]) -> Result<CDemoConsoleCmd, DecodeCmdError>;
    // fn decode_cmd_custom_data(data: &[u8]) -> Result<CDemoCustomData, DecodeCmdError>;
    // fn decode_cmd_custom_data_callbacks(data: &[u8]) -> Result<CDemoCustomDataCallbacks, DecodeCmdError>;
    // fn decode_cmd_user_cmd(data: &[u8]) -> Result<CDemoUserCmd, DecodeCmdError>;
    fn decode_cmd_full_packet(data: &[u8]) -> Result<CDemoFullPacket, DecodeCmdError>;
    // fn decode_cmd_save_game(data: &[u8]) -> Result<CDemoSaveGame, DecodeCmdError>;
    // fn decode_cmd_spawn_groups(data: &[u8]) -> Result<CDemoSpawnGroups, DecodeCmdError>;
    // fn decode_cmd_animation_data(data: &[u8]) -> Result<CDemoAnimationData, DecodeCmdError>;
    // fn decode_cmd_animation_header(data: &[u8]) -> Result<CDemoAnimationHeader, DecodeCmdError>;
    // Max
    // IsCompressed (flag)

    fn skip_cmd(&mut self, cmd_header: &CmdHeader) -> Result<(), io::Error> {
        self.seek(SeekFrom::Current(i64::from(cmd_header.body_size)))
            .map(|_| ())
    }

    // other
    // ----

    fn start_position(&self) -> u64;

    // TODO: how not cool is it to rely on anyhow here?
    fn total_ticks(&mut self) -> Result<i32, anyhow::Error>;
}
