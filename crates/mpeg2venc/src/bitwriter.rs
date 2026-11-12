/// MSB-first bit writer with MPEG-2 start-code alignment.
///
/// MPEG-2 has no emulation prevention bytes (that is H.264): the syntax itself avoids start-code emulation.
pub(crate) struct BitWriter {
    output: Vec<u8>,
    bit_buffer: u8,
    bit_count: u8,
}

impl BitWriter {
    /// Create an empty bit writer.
    pub(crate) fn new() -> Self {
        Self {
            output: Vec::new(),
            bit_buffer: 0,
            bit_count: 0,
        }
    }

    /// Write one bit.
    pub(crate) fn write_bit(&mut self, bit: bool) {
        if bit {
            self.bit_buffer |= 1 << (7 - self.bit_count);
        }
        self.bit_count += 1;
        if self.bit_count == 8 {
            self.flush_byte();
        }
    }

    /// Write `len` bits from `value`, most significant bit first.
    pub(crate) fn write_bits(&mut self, value: u32, len: u8) {
        let mut remaining = len;
        while remaining > 0 {
            remaining -= 1;
            let bit = (value >> u32::from(remaining)) & 1 != 0;
            self.write_bit(bit);
        }
    }

    /// Stuff zero bits until the next byte boundary.
    pub(crate) fn align_zero(&mut self) {
        if self.bit_count != 0 {
            self.flush_byte();
        }
    }

    /// Align to a byte boundary and write a raw MPEG-2 start code.
    pub(crate) fn write_start_code(&mut self, code: u8) {
        self.align_zero();
        self.push_raw_byte(0x00);
        self.push_raw_byte(0x00);
        self.push_raw_byte(0x01);
        self.push_raw_byte(code);
    }

    /// Flush any partial byte and return the encoded bytes.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        self.align_zero();
        self.output
    }

    fn flush_byte(&mut self) {
        self.push_data_byte(self.bit_buffer);
        self.bit_buffer = 0;
        self.bit_count = 0;
    }

    fn push_data_byte(&mut self, byte: u8) {
        self.output.push(byte);
    }

    fn push_raw_byte(&mut self, byte: u8) {
        self.output.push(byte);
    }
}

#[cfg(test)]
mod tests {
    use super::BitWriter;

    #[test]
    fn writes_msb_first() {
        let mut writer = BitWriter::new();
        writer.write_bits(0b101, 3);
        writer.write_bit(true);
        writer.write_bits(0b10, 2);
        let bytes = writer.finish();
        assert_eq!(bytes, vec![0b10111000]);
    }

    #[test]
    fn aligns_before_start_code() {
        let mut writer = BitWriter::new();
        writer.write_bits(1, 1);
        writer.align_zero();
        writer.write_start_code(0xB3);
        let bytes = writer.finish();
        assert_eq!(bytes, vec![0x80, 0x00, 0x00, 0x01, 0xB3]);
    }

    #[test]
    fn start_code_after_partial_byte() {
        let mut writer = BitWriter::new();
        writer.write_bits(0b11, 2);
        writer.write_start_code(0x00);
        let bytes = writer.finish();
        assert_eq!(bytes, vec![0b11000000, 0x00, 0x00, 0x01, 0x00]);
    }

    #[test]
    fn no_emulation_prevention() {
        let mut writer = BitWriter::new();
        writer.write_bits(0, 8);
        writer.write_bits(0, 8);
        writer.write_bits(1, 8);
        let bytes = writer.finish();
        assert_eq!(bytes, vec![0x00, 0x00, 0x01]);
    }
}
