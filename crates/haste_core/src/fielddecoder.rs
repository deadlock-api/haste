use core::fmt::Debug;
use core::str::Utf8Error;
use dungers::bitbuf::BitError;
use dyn_clone::DynClone;

use crate::bitreader::BitReader;
use crate::fieldvalue::FieldValue;
use crate::flattenedserializers::FlattenedSerializerField;
use crate::fxhash;
use crate::quantizedfloat::{QuantizedFloat, QuantizedFloatError};

// NOTE: PropTypeFns (from csgo source code) is what you are looking for, it has all the encoders,
// decoders, proxies and all of the stuff.

#[derive(thiserror::Error, Debug)]
pub enum DecoderError {
    #[error(transparent)]
    QuantizedFloat(#[from] QuantizedFloatError),
    #[error(transparent)]
    Bit(#[from] BitError),
    #[error(transparent)]
    UTF8(#[from] Utf8Error),
}

// ----

// public/dt_common.h
const DT_MAX_STRING_BITS: u32 = 9;
/// maximum length of a string that can be sent.
const DT_MAX_STRING_BUFFERSIZE: u32 = 1 << DT_MAX_STRING_BITS;

#[derive(Debug)]
pub(crate) struct FieldDecodeContext {
    pub(crate) tick_interval: f32,
    pub(crate) string_buf: [u8; DT_MAX_STRING_BUFFERSIZE as usize],
}

impl Default for FieldDecodeContext {
    fn default() -> Self {
        Self {
            // NOTE(blukai): tick interval needs to be read from SvcServerInfo packet message. it
            // becomes available "later"; it is okay to initialize it to 0.0.
            tick_interval: 0.0,
            string_buf: [0u8; DT_MAX_STRING_BUFFERSIZE as usize],
        }
    }
}

// ----

// TODO: get rid of trait objects; find a better, more efficient, way to
// "attach" decoders to fields; but note that having separate decoding functions
// and attaching function "pointers" to fields is even worse.

// TODO(blukai): try to not box internal decoders (for example u64).

pub(crate) trait FieldDecode: DynClone + Debug + Send + Sync {
    fn decode(
        &self,
        ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError>;
}

dyn_clone::clone_trait_object!(FieldDecode);

/// used during multi-phase initialization. never called.
#[derive(Debug, Clone, Default)]
pub(crate) struct InvalidDecoder;

impl FieldDecode for InvalidDecoder {
    #[cold]
    fn decode(
        &self,
        _ctx: &mut FieldDecodeContext,
        _br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        unreachable!()
    }
}

// ----

trait InternalFieldDecode<T>: DynClone + Debug + Send + Sync {
    fn decode(&self, ctx: &mut FieldDecodeContext, br: &mut BitReader) -> Result<T, BitError>;
}

dyn_clone::clone_trait_object!(<T> InternalFieldDecode<T>);

// ----

#[derive(Debug, Clone, Default)]
pub(crate) struct I64Decoder;

impl FieldDecode for I64Decoder {
    fn decode(
        &self,
        _ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        Ok(FieldValue::I64(br.read_varint64()?))
    }
}

// ----

#[derive(Debug, Clone, Default)]
struct InternalU64Decoder;

impl FieldDecode for InternalU64Decoder {
    fn decode(
        &self,
        _ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        Ok(FieldValue::U64(br.read_uvarint64()?))
    }
}

#[derive(Debug, Clone, Default)]
struct InternalU64Fixed64Decoder;

impl FieldDecode for InternalU64Fixed64Decoder {
    fn decode(
        &self,
        _ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        let mut buf = [0u8; 8];
        br.read_bytes(&mut buf)?;
        Ok(FieldValue::U64(u64::from_le_bytes(buf)))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct U64Decoder {
    decoder: Box<dyn FieldDecode>,
}

// NOTE: default should only be used to decode dynamic array lengths. for everything else decoder
// must be constructed using U64Decoder's new method.
impl Default for U64Decoder {
    fn default() -> Self {
        Self {
            decoder: Box::<InternalU64Decoder>::default(),
        }
    }
}

impl U64Decoder {
    pub(crate) fn new(field: &FlattenedSerializerField) -> Self {
        if field.var_encoder_heq(fxhash::hash_bytes(b"fixed64")) {
            Self {
                decoder: Box::<InternalU64Fixed64Decoder>::default(),
            }
        } else {
            Self::default()
        }
    }
}

impl FieldDecode for U64Decoder {
    fn decode(
        &self,
        ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        self.decoder.decode(ctx, br)
    }
}

// ----

#[derive(Debug, Clone, Default)]
pub(crate) struct BoolDecoder;

impl FieldDecode for BoolDecoder {
    fn decode(
        &self,
        _ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        Ok(FieldValue::Bool(br.read_bool()?))
    }
}

// ----

#[derive(Debug, Clone, Default)]
pub(crate) struct StringDecoder;

impl FieldDecode for StringDecoder {
    fn decode(
        &self,
        ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        let n = br.read_string(&mut ctx.string_buf, false)?;
        // TODO(blukai): should string conversion be actually checked? why not?
        Ok(FieldValue::String(Box::<str>::from(core::str::from_utf8(
            &ctx.string_buf[..n],
        )?)))
    }
}

// ----

#[derive(Debug, Clone, Default)]
struct InternalF32SimulationTimeDecoder;

impl InternalFieldDecode<f32> for InternalF32SimulationTimeDecoder {
    fn decode(&self, ctx: &mut FieldDecodeContext, br: &mut BitReader) -> Result<f32, BitError> {
        Ok(br.read_uvarint32()? as f32 * ctx.tick_interval)
    }
}

#[derive(Debug, Clone, Default)]
struct InternalF32CoordDecoder;

impl InternalFieldDecode<f32> for InternalF32CoordDecoder {
    fn decode(&self, _ctx: &mut FieldDecodeContext, br: &mut BitReader) -> Result<f32, BitError> {
        br.read_bitcoord()
    }
}

#[derive(Debug, Clone, Default)]
struct InternalF32NormalDecoder;

impl InternalFieldDecode<f32> for InternalF32NormalDecoder {
    fn decode(&self, _ctx: &mut FieldDecodeContext, br: &mut BitReader) -> Result<f32, BitError> {
        br.read_bitnormal()
    }
}

#[derive(Debug, Clone, Default)]
struct InternalF32NoScaleDecoder;

impl InternalFieldDecode<f32> for InternalF32NoScaleDecoder {
    fn decode(&self, _ctx: &mut FieldDecodeContext, br: &mut BitReader) -> Result<f32, BitError> {
        br.read_bitfloat()
    }
}

#[derive(Debug, Clone)]
struct InternalQuantizedFloatDecoder {
    quantized_float: QuantizedFloat,
}

impl InternalQuantizedFloatDecoder {
    pub(crate) fn new(field: &FlattenedSerializerField) -> Result<Self, DecoderError> {
        Ok(Self {
            quantized_float: QuantizedFloat::new(
                field.bit_count.unwrap_or_default(),
                field.encode_flags.unwrap_or_default(),
                field.low_value.unwrap_or_default(),
                field.high_value.unwrap_or_default(),
            )?,
        })
    }
}

impl InternalFieldDecode<f32> for InternalQuantizedFloatDecoder {
    fn decode(&self, _ctx: &mut FieldDecodeContext, br: &mut BitReader) -> Result<f32, BitError> {
        self.quantized_float.decode(br)
    }
}

// ----

#[derive(Debug, Clone)]
pub(crate) struct InternalF32Decoder {
    decoder: Box<dyn InternalFieldDecode<f32>>,
}

impl InternalF32Decoder {
    pub(crate) fn new(field: &FlattenedSerializerField) -> Result<Self, DecoderError> {
        if field.var_name.hash == fxhash::hash_bytes(b"m_flSimulationTime")
            || field.var_name.hash == fxhash::hash_bytes(b"m_flAnimTime")
        {
            return Ok(Self {
                decoder: Box::<InternalF32SimulationTimeDecoder>::default(),
            });
        }

        if let Some(var_encoder) = field.var_encoder.as_ref() {
            match var_encoder.hash {
                hash if hash == fxhash::hash_bytes(b"coord") => {
                    return Ok(Self {
                        decoder: Box::<InternalF32CoordDecoder>::default(),
                    });
                }
                hash if hash == fxhash::hash_bytes(b"normal") => {
                    return Ok(Self {
                        decoder: Box::<InternalF32NormalDecoder>::default(),
                    });
                }
                _ => unimplemented!("{:?}", var_encoder),
            }
        }

        let bit_count = field.bit_count.unwrap_or_default();
        // NOTE: that would mean that something is seriously wrong - in that case yell at me
        // loudly.
        debug_assert!((0..=32).contains(&bit_count));
        if bit_count == 0 || bit_count == 32 {
            return Ok(Self {
                decoder: Box::<InternalF32NoScaleDecoder>::default(),
            });
        }

        Ok(Self {
            decoder: Box::new(InternalQuantizedFloatDecoder::new(field)?),
        })
    }
}

impl InternalFieldDecode<f32> for InternalF32Decoder {
    fn decode(&self, ctx: &mut FieldDecodeContext, br: &mut BitReader) -> Result<f32, BitError> {
        self.decoder.decode(ctx, br)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct F32Decoder {
    decoder: Box<dyn InternalFieldDecode<f32>>,
}

impl F32Decoder {
    pub(crate) fn new(field: &FlattenedSerializerField) -> Result<Self, DecoderError> {
        Ok(Self {
            decoder: Box::new(InternalF32Decoder::new(field)?),
        })
    }
}

impl FieldDecode for F32Decoder {
    fn decode(
        &self,
        ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        Ok(FieldValue::F32(self.decoder.decode(ctx, br)?))
    }
}

// ----

#[derive(Debug, Clone)]
struct InternalVector3DefaultDecoder {
    decoder: Box<dyn InternalFieldDecode<f32>>,
}

impl FieldDecode for InternalVector3DefaultDecoder {
    fn decode(
        &self,
        ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        let vec3 = [
            self.decoder.decode(ctx, br)?,
            self.decoder.decode(ctx, br)?,
            self.decoder.decode(ctx, br)?,
        ];
        Ok(FieldValue::Vector3(vec3))
    }
}

#[derive(Debug, Clone, Default)]
struct InternalVector3NormalDecoder;

impl FieldDecode for InternalVector3NormalDecoder {
    fn decode(
        &self,
        _ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        Ok(FieldValue::Vector3(br.read_bitvec3normal()?))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Vector3Decoder {
    decoder: Box<dyn FieldDecode>,
}

impl Vector3Decoder {
    pub(crate) fn new(field: &FlattenedSerializerField) -> Result<Self, DecoderError> {
        if field.var_encoder_heq(fxhash::hash_bytes(b"normal")) {
            Ok(Self {
                decoder: Box::<InternalVector3NormalDecoder>::default(),
            })
        } else {
            Ok(Self {
                decoder: Box::new(InternalVector3DefaultDecoder {
                    decoder: Box::new(InternalF32Decoder::new(field)?),
                }),
            })
        }
    }
}

impl FieldDecode for Vector3Decoder {
    fn decode(
        &self,
        ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        self.decoder.decode(ctx, br)
    }
}

// ----

#[derive(Debug, Clone)]
pub(crate) struct Vector2Decoder {
    decoder: Box<dyn InternalFieldDecode<f32>>,
}

impl Vector2Decoder {
    pub(crate) fn new(field: &FlattenedSerializerField) -> Result<Self, DecoderError> {
        Ok(Self {
            decoder: Box::new(InternalF32Decoder::new(field)?),
        })
    }
}

impl FieldDecode for Vector2Decoder {
    fn decode(
        &self,
        ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        let vec2 = [self.decoder.decode(ctx, br)?, self.decoder.decode(ctx, br)?];
        Ok(FieldValue::Vector2(vec2))
    }
}

// ----

#[derive(Debug, Clone)]
pub(crate) struct Vector4Decoder {
    decoder: Box<dyn InternalFieldDecode<f32>>,
}

impl Vector4Decoder {
    pub(crate) fn new(field: &FlattenedSerializerField) -> Result<Self, DecoderError> {
        Ok(Self {
            decoder: Box::new(InternalF32Decoder::new(field)?),
        })
    }
}

impl FieldDecode for Vector4Decoder {
    fn decode(
        &self,
        ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        let vec4 = [
            self.decoder.decode(ctx, br)?,
            self.decoder.decode(ctx, br)?,
            self.decoder.decode(ctx, br)?,
            self.decoder.decode(ctx, br)?,
        ];
        Ok(FieldValue::Vector4(vec4))
    }
}

// ----

#[derive(Debug, Clone)]
struct InternalQAnglePitchYawDecoder {
    bit_count: usize,
}

impl FieldDecode for InternalQAnglePitchYawDecoder {
    fn decode(
        &self,
        _ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        let vec3 = [
            br.read_bitangle(self.bit_count)?,
            br.read_bitangle(self.bit_count)?,
            0.0,
        ];
        Ok(FieldValue::QAngle(vec3))
    }
}

#[derive(Debug, Clone, Default)]
struct InternalQAngleNoBitCountDecoder;

impl FieldDecode for InternalQAngleNoBitCountDecoder {
    fn decode(
        &self,
        _ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        Ok(FieldValue::QAngle(br.read_bitvec3coord()?))
    }
}

#[derive(Debug, Clone, Default)]
struct InternalQAnglePreciseDecoder;

impl FieldDecode for InternalQAnglePreciseDecoder {
    fn decode(
        &self,
        _ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        let mut vec3 = [0f32; 3];

        let rx = br.read_bool()?;
        let ry = br.read_bool()?;
        let rz = br.read_bool()?;

        if rx {
            vec3[0] = br.read_bitangle(20)?;
        }
        if ry {
            vec3[1] = br.read_bitangle(20)?;
        }
        if rz {
            vec3[2] = br.read_bitangle(20)?;
        }

        Ok(FieldValue::QAngle(vec3))
    }
}

#[derive(Debug, Clone)]
struct InternalQAngleBitCountDecoder {
    bit_count: usize,
}

impl FieldDecode for InternalQAngleBitCountDecoder {
    fn decode(
        &self,
        _ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        let vec3 = [
            br.read_bitangle(self.bit_count)?,
            br.read_bitangle(self.bit_count)?,
            br.read_bitangle(self.bit_count)?,
        ];
        Ok(FieldValue::QAngle(vec3))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct QAngleDecoder {
    decoder: Box<dyn FieldDecode>,
}

impl QAngleDecoder {
    pub(crate) fn new(field: &FlattenedSerializerField) -> Self {
        let bit_count = field.bit_count.unwrap_or_default() as usize;

        if let Some(var_encoder) = field.var_encoder.as_ref() {
            match var_encoder.hash {
                hash if hash == fxhash::hash_bytes(b"qangle_pitch_yaw") => {
                    return Self {
                        decoder: Box::new(InternalQAnglePitchYawDecoder { bit_count }),
                    };
                }
                hash if hash == fxhash::hash_bytes(b"qangle_precise") => {
                    return Self {
                        decoder: Box::<InternalQAnglePreciseDecoder>::default(),
                    };
                }

                hash if hash == fxhash::hash_bytes(b"qangle") => {}
                // NOTE(blukai): naming of var encoders seem inconsistent. found this pascal cased
                // name in dota 2 replay from 2018.
                hash if hash == fxhash::hash_bytes(b"QAngle") => {}

                _ => unimplemented!("{:?}", var_encoder),
            }
        }

        if bit_count == 0 {
            return Self {
                decoder: Box::<InternalQAngleNoBitCountDecoder>::default(),
            };
        }

        Self {
            decoder: Box::new(InternalQAngleBitCountDecoder { bit_count }),
        }
    }
}

impl FieldDecode for QAngleDecoder {
    fn decode(
        &self,
        ctx: &mut FieldDecodeContext,
        br: &mut BitReader,
    ) -> Result<FieldValue, DecoderError> {
        self.decoder.decode(ctx, br)
    }
}
