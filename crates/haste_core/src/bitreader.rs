pub use bitbuf::OverflowError as BitReaderOverflowError;
use dungers::bitbuf;

#[derive(thiserror::Error, Debug)]
pub enum BitReaderError {
    #[error("Buffer overflow: attempted to read beyond available data")]
    BufferOverflow(#[from] bitbuf::OverflowError),
    #[error("Read into buffer error")]
    ReadIntoBuffer(#[from] bitbuf::ReadIntoBufferError),
    #[error("Read varint error")]
    ReadVarint(#[from] bitbuf::ReadVarintError),
    #[error("Buffer too small: need at least {needed} bytes, but buffer has {available}")]
    BufferTooSmall { needed: usize, available: usize },
    #[error("Invalid string: buffer is empty")]
    EmptyBuffer,
    #[error("String too long: string length {length} exceeds buffer capacity {capacity}")]
    StringTooLong { length: usize, capacity: usize },
}

// public/coordsize.h
const COORD_INTEGER_BITS: usize = 14;
const COORD_FRACTIONAL_BITS: usize = 5;
const COORD_DENOMINATOR: f32 = (1 << COORD_FRACTIONAL_BITS) as f32;
const COORD_RESOLUTION: f32 = 1.0 / COORD_DENOMINATOR;

// public/coordsize.h
const NORMAL_FRACTIONAL_BITS: usize = 11;
const NORMAL_DENOMINATOR: f32 = ((1 << (NORMAL_FRACTIONAL_BITS)) - 1) as f32;
const NORMAL_RESOLUTION: f32 = 1.0 / (NORMAL_DENOMINATOR);

// BitRead is a port of valve's CBitRead(or/and old_bf_read) from valve's tier1 lib.
pub struct BitReader<'a> {
    inner: bitbuf::BitReader<'a>,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            inner: bitbuf::BitReader::new(data),
        }
    }

    /// delegated from [dungers::bitbuf::BitReader].
    pub fn num_bits_left(&self) -> usize {
        self.inner.num_bits_left()
    }

    /// delegated from [dungers::bitbuf::BitReader].
    pub fn read_ubit64(&mut self, num_bits: usize) -> Result<u64, BitReaderError> {
        self.inner
            .read_ubit64(num_bits)
            .map_err(BitReaderError::from)
    }

    /// delegated from [dungers::bitbuf::BitReader].
    pub fn read_bool(&mut self) -> Result<bool, BitReaderError> {
        self.inner.read_bool().map_err(BitReaderError::from)
    }

    /// delegated from [dungers::bitbuf::BitReader].
    pub fn read_byte(&mut self) -> Result<u8, BitReaderError> {
        self.inner.read_byte().map_err(BitReaderError::from)
    }

    /// delegated from [dungers::bitbuf::BitReader].
    pub fn read_bits(&mut self, buf: &mut [u8], num_bits: usize) -> Result<(), BitReaderError> {
        self.inner
            .read_bits(buf, num_bits)
            .map_err(BitReaderError::from)
    }

    /// delegated from [dungers::bitbuf::BitReader].
    pub fn read_bytes(&mut self, buf: &mut [u8]) -> Result<(), BitReaderError> {
        self.inner.read_bytes(buf).map_err(BitReaderError::from)
    }

    /// delegated from [dungers::bitbuf::BitReader].
    pub fn read_uvarint32(&mut self) -> Result<u32, BitReaderError> {
        self.inner.read_uvarint32().map_err(BitReaderError::from)
    }

    /// delegated from [dungers::bitbuf::BitReader].
    pub fn read_uvarint64(&mut self) -> Result<u64, BitReaderError> {
        self.inner.read_uvarint64().map_err(BitReaderError::from)
    }

    /// delegated from [dungers::bitbuf::BitReader].
    pub fn read_varint32(&mut self) -> Result<i32, BitReaderError> {
        self.inner.read_varint32().map_err(BitReaderError::from)
    }

    /// delegated from [dungers::bitbuf::BitReader].
    pub fn read_varint64(&mut self) -> Result<i64, BitReaderError> {
        self.inner.read_varint64().map_err(BitReaderError::from)
    }

    // ubitvar is "valve's own variable-length integer encoding" (c) butterfly.
    //
    // valve's refs:
    // - [1] https://github.com/ValveSoftware/csgo-demoinfo/blob/049f8dbf49099d3cc544ec5061a7f7252cce7b82/demoinfogo/demofilebitbuf.cpp#L171
    // - [2]: https://github.com/ValveSoftware/source-sdk-2013/blob/0d8dceea4310fde5706b3ce1c70609d72a38efdf/sp/src/public/tier1/bitbuf.h#L756
    //
    // NOTE: butterfly, manta and clarity - all have same exact implementation.
    //
    // quote from clarity:
    // Thanks to Robin Dietrich for providing a clean version of this code :-)
    // The header looks like this: [XY00001111222233333333333333333333] where everything > 0 is optional.
    // The first 2 bits (X and Y) tell us how much (if any) to read other than the 6 initial bits:
    // Y set -> read 4
    // X set -> read 8
    // X + Y set -> read 28
    pub fn read_ubitvar(&mut self) -> Result<u32, BitReaderError> {
        let ret = self.read_ubit64(6)?;
        let ret = match ret & (16 | 32) {
            16 => (ret & 15) | (self.read_ubit64(4)? << 4),
            32 => (ret & 15) | (self.read_ubit64(8)? << 4),
            48 => (ret & 15) | (self.read_ubit64(32 - 4)? << 4),
            _ => ret,
        };
        Ok(ret as u32)
    }

    pub fn read_bitfloat(&mut self) -> Result<f32, BitReaderError> {
        Ok(f32::from_bits(self.read_ubit64(32)? as u32))
    }

    pub fn read_bitcoord(&mut self) -> Result<f32, BitReaderError> {
        let mut value: f32 = 0.0;

        // Read the required integer and fraction flags
        let has_intval = self.read_bool()?;
        let has_fractval = self.read_bool()?;

        // If we got either parse them, otherwise it's a zero.
        if has_intval || has_fractval {
            // Read the sign bit
            let signbit = self.read_bool()?;

            // If there's an integer, read it in
            let mut intval = 0;
            if has_intval {
                // Adjust the integers from [0..MAX_COORD_VALUE-1] to [1..MAX_COORD_VALUE]
                intval = self.read_ubit64(COORD_INTEGER_BITS)? + 1;
            }

            // If there's a fraction, read it in
            let mut fractval = 0;
            if has_fractval {
                fractval = self.read_ubit64(COORD_FRACTIONAL_BITS)?;
            }

            // Calculate the correct floating point value
            value = intval as f32 + (fractval as f32 * COORD_RESOLUTION);

            // Fixup the sign if negative.
            if signbit {
                value = -value;
            }
        }

        Ok(value)
    }

    pub fn read_bitnormal(&mut self) -> Result<f32, BitReaderError> {
        // read the sign bit
        let signbit = self.read_bool()?;

        // read the fractional part
        let fractval = self.read_ubit64(NORMAL_FRACTIONAL_BITS)?;

        // calculate the correct floating point value
        let mut value = fractval as f32 * NORMAL_RESOLUTION;

        // fixup the sign if negative.
        if signbit {
            value = -value;
        }

        Ok(value)
    }

    pub fn read_bitvec3coord(&mut self) -> Result<[f32; 3], BitReaderError> {
        let mut fa = [0f32; 3];

        let xflag = self.read_bool()?;
        let yflag = self.read_bool()?;
        let zflag = self.read_bool()?;

        if xflag {
            fa[0] = self.read_bitcoord()?;
        }
        if yflag {
            fa[1] = self.read_bitcoord()?;
        }
        if zflag {
            fa[2] = self.read_bitcoord()?;
        }

        Ok(fa)
    }

    pub fn read_bitvec3normal(&mut self) -> Result<[f32; 3], BitReaderError> {
        let mut fa = [0f32; 3];

        let xflag = self.read_bool()?;
        let yflag = self.read_bool()?;

        if xflag {
            fa[0] = self.read_bitnormal()?;
        }
        if yflag {
            fa[1] = self.read_bitnormal()?;
        }

        // the first two imply the third (but not its sign)
        let znegative = self.read_bool()?;

        let fafafbfb = fa[0] * fa[0] + fa[1] * fa[1];
        if fafafbfb < 1.0 {
            fa[2] = (1.0 - fafafbfb).sqrt();
        }

        if znegative {
            fa[2] = -fa[2];
        }

        Ok(fa)
    }

    pub fn read_bitangle(&mut self, num_bits: usize) -> Result<f32, BitReaderError> {
        let shift = bitbuf::get_bit_for_bit_num(num_bits) as f32;

        let u = self.read_ubit64(num_bits)?;
        Ok((u as f32) * (360.0 / shift))
    }

    // Always reads to the end of the string (so you can read the next piece of data waiting).
    //
    // If line is true, it stops when it reaches a '\n' or a null-terminator.
    //
    // buf is always null-terminated (unless buf.len() is 0).
    //
    // Returns the number of characters left in out when the routine is complete (this will never
    // exceed buf.len()-1).
    pub fn read_string(&mut self, buf: &mut [u8], line: bool) -> Result<usize, BitReaderError> {
        if buf.is_empty() {
            return Err(BitReaderError::EmptyBuffer);
        }

        let mut too_small = false;
        let mut num_chars = 0;
        loop {
            let val = self.read_byte()?;
            if val == 0 || (line && val == b'\n') {
                break;
            }

            if num_chars < (buf.len() - 1) {
                buf[num_chars] = val;
                num_chars += 1;
            } else {
                too_small = true;
            }
        }

        // make sure it's null-terminated.
        if num_chars >= buf.len() {
            return Err(BitReaderError::BufferTooSmall {
                needed: num_chars + 1,
                available: buf.len(),
            });
        }
        buf[num_chars] = 0;

        // did it fit?
        if too_small {
            return Err(BitReaderError::StringTooLong {
                length: num_chars,
                capacity: buf.len() - 1,
            });
        }

        Ok(num_chars)
    }

    pub fn read_ubitvarfp(&mut self) -> Result<u32, BitReaderError> {
        let ret = if self.read_bool()? {
            self.read_ubit64(2)?
        } else if self.read_bool()? {
            self.read_ubit64(4)?
        } else if self.read_bool()? {
            self.read_ubit64(10)?
        } else if self.read_bool()? {
            self.read_ubit64(17)?
        } else {
            self.read_ubit64(31)?
        };
        Ok(ret as u32)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_read_string() {
        let buf = b"Life's but a walking shadow, a poor player.\0";
        let mut br = BitReader::new(buf);

        let mut out = vec![0u8; buf.len()];
        let num_chars = br
            .read_string(&mut out, false)
            .expect("Failed to read string");
        assert_eq!(&out, &buf);
        assert_eq!(num_chars, buf.len() - 1);
    }
}
