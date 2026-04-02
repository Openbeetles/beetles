//! 飞书长连接 WebSocket 帧格式（pbbp2）。
//! 为缩小固件体积，这里只实现当前协议实际使用的最小 protobuf 子集。

use crate::error::{Error, Result};

pub mod pbbp2 {
    use super::{
        decode_length_delimited, decode_length_delimited_bytes, decode_varint, encode_key,
        encode_len_delimited_bytes,
    };
    use crate::error::{Error, Result};

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Header {
        pub key: String,
        pub value: String,
    }

    impl Header {
        fn encode_into(&self, out: &mut Vec<u8>) {
            encode_len_delimited_bytes(1, self.key.as_bytes(), out);
            encode_len_delimited_bytes(2, self.value.as_bytes(), out);
        }

        fn decode(input: &[u8]) -> Result<Self> {
            let mut header = Header::default();
            let mut pos = 0usize;
            while pos < input.len() {
                let key = decode_varint(input, &mut pos, "feishu_ws_frame_decode")?;
                let field = (key >> 3) as u32;
                let wire_type = (key & 0x07) as u8;
                match field {
                    1 => {
                        header.key = decode_length_delimited(
                            input,
                            &mut pos,
                            wire_type,
                            "feishu_ws_frame_decode",
                        )?;
                    }
                    2 => {
                        header.value = decode_length_delimited(
                            input,
                            &mut pos,
                            wire_type,
                            "feishu_ws_frame_decode",
                        )?;
                    }
                    _ => skip_field(input, &mut pos, wire_type)?,
                }
            }
            Ok(header)
        }
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Frame {
        pub seq_id: u64,
        pub log_id: u64,
        pub service: i32,
        pub method: i32,
        pub headers: Vec<Header>,
        pub payload_encoding: String,
        pub payload_type: String,
        pub payload: Vec<u8>,
        pub log_id_new: String,
    }

    impl Frame {
        pub fn encode_to_vec(&self) -> Vec<u8> {
            let mut out = Vec::with_capacity(64 + self.payload.len());
            encode_varint_field(1, self.seq_id, &mut out);
            encode_varint_field(2, self.log_id, &mut out);
            encode_varint_field(3, self.service.max(0) as u64, &mut out);
            encode_varint_field(4, self.method.max(0) as u64, &mut out);
            for header in &self.headers {
                let mut nested = Vec::with_capacity(header.key.len() + header.value.len() + 8);
                header.encode_into(&mut nested);
                encode_len_delimited_bytes(5, &nested, &mut out);
            }
            if !self.payload_encoding.is_empty() {
                encode_len_delimited_bytes(6, self.payload_encoding.as_bytes(), &mut out);
            }
            if !self.payload_type.is_empty() {
                encode_len_delimited_bytes(7, self.payload_type.as_bytes(), &mut out);
            }
            if !self.payload.is_empty() {
                encode_len_delimited_bytes(8, &self.payload, &mut out);
            }
            if !self.log_id_new.is_empty() {
                encode_len_delimited_bytes(9, self.log_id_new.as_bytes(), &mut out);
            }
            out
        }

        pub fn decode(input: &[u8]) -> Result<Self> {
            let mut frame = Frame::default();
            let mut pos = 0usize;
            while pos < input.len() {
                let key = decode_varint(input, &mut pos, "feishu_ws_frame_decode")?;
                let field = (key >> 3) as u32;
                let wire_type = (key & 0x07) as u8;
                match field {
                    1 => frame.seq_id = decode_varint_field(input, &mut pos, wire_type)?,
                    2 => frame.log_id = decode_varint_field(input, &mut pos, wire_type)?,
                    3 => frame.service = decode_i32_field(input, &mut pos, wire_type)?,
                    4 => frame.method = decode_i32_field(input, &mut pos, wire_type)?,
                    5 => {
                        let nested = decode_length_delimited_bytes(
                            input,
                            &mut pos,
                            wire_type,
                            "feishu_ws_frame_decode",
                        )?;
                        frame.headers.push(Header::decode(nested)?);
                    }
                    6 => {
                        frame.payload_encoding = decode_length_delimited(
                            input,
                            &mut pos,
                            wire_type,
                            "feishu_ws_frame_decode",
                        )?;
                    }
                    7 => {
                        frame.payload_type = decode_length_delimited(
                            input,
                            &mut pos,
                            wire_type,
                            "feishu_ws_frame_decode",
                        )?;
                    }
                    8 => {
                        frame.payload = decode_length_delimited_bytes(
                            input,
                            &mut pos,
                            wire_type,
                            "feishu_ws_frame_decode",
                        )?
                        .to_vec();
                    }
                    9 => {
                        frame.log_id_new = decode_length_delimited(
                            input,
                            &mut pos,
                            wire_type,
                            "feishu_ws_frame_decode",
                        )?;
                    }
                    _ => skip_field(input, &mut pos, wire_type)?,
                }
            }
            Ok(frame)
        }
    }

    fn encode_varint_field(field: u32, value: u64, out: &mut Vec<u8>) {
        encode_key(field, 0, out);
        super::encode_varint(value, out);
    }

    fn decode_varint_field(input: &[u8], pos: &mut usize, wire_type: u8) -> Result<u64> {
        if wire_type != 0 {
            return Err(Error::config(
                "feishu_ws_frame_decode",
                format!("expected varint wire type, got {}", wire_type),
            ));
        }
        decode_varint(input, pos, "feishu_ws_frame_decode")
    }

    fn decode_i32_field(input: &[u8], pos: &mut usize, wire_type: u8) -> Result<i32> {
        let value = decode_varint_field(input, pos, wire_type)?;
        i32::try_from(value).map_err(|_| {
            Error::config(
                "feishu_ws_frame_decode",
                format!("int32 field out of range: {}", value),
            )
        })
    }

    fn skip_field(input: &[u8], pos: &mut usize, wire_type: u8) -> Result<()> {
        match wire_type {
            0 => {
                let _ = decode_varint(input, pos, "feishu_ws_frame_decode")?;
                Ok(())
            }
            1 => advance(input, pos, 8),
            2 => {
                let len = decode_varint(input, pos, "feishu_ws_frame_decode")? as usize;
                advance(input, pos, len)
            }
            5 => advance(input, pos, 4),
            _ => Err(Error::config(
                "feishu_ws_frame_decode",
                format!("unsupported wire type: {}", wire_type),
            )),
        }
    }

    fn advance(input: &[u8], pos: &mut usize, len: usize) -> Result<()> {
        let next = pos.saturating_add(len);
        if next > input.len() {
            return Err(Error::config(
                "feishu_ws_frame_decode",
                "protobuf field exceeds frame length",
            ));
        }
        *pos = next;
        Ok(())
    }
}

fn encode_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn encode_key(field: u32, wire_type: u8, out: &mut Vec<u8>) {
    encode_varint(((field as u64) << 3) | wire_type as u64, out);
}

fn encode_len_delimited_bytes(field: u32, value: &[u8], out: &mut Vec<u8>) {
    encode_key(field, 2, out);
    encode_varint(value.len() as u64, out);
    out.extend_from_slice(value);
}

fn decode_varint(input: &[u8], pos: &mut usize, stage: &'static str) -> Result<u64> {
    let mut shift = 0u32;
    let mut value = 0u64;
    loop {
        let byte = *input
            .get(*pos)
            .ok_or_else(|| Error::config(stage, "unexpected EOF while decoding protobuf varint"))?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift >= 64 {
            return Err(Error::config(stage, "protobuf varint is too long"));
        }
    }
}

fn decode_length_delimited<'a>(
    input: &'a [u8],
    pos: &mut usize,
    wire_type: u8,
    stage: &'static str,
) -> Result<String> {
    let raw = decode_length_delimited_bytes(input, pos, wire_type, stage)?;
    std::str::from_utf8(raw)
        .map(|s| s.to_string())
        .map_err(|e| Error::Other {
            source: Box::new(e),
            stage,
        })
}

fn decode_length_delimited_bytes<'a>(
    input: &'a [u8],
    pos: &mut usize,
    wire_type: u8,
    stage: &'static str,
) -> Result<&'a [u8]> {
    if wire_type != 2 {
        return Err(Error::config(
            stage,
            format!("expected len-delimited wire type, got {}", wire_type),
        ));
    }
    let len = decode_varint(input, pos, stage)? as usize;
    let end = pos.saturating_add(len);
    if end > input.len() {
        return Err(Error::config(stage, "protobuf field exceeds frame length"));
    }
    let slice = &input[*pos..end];
    *pos = end;
    Ok(slice)
}

#[cfg(test)]
mod tests {
    use super::pbbp2::{Frame, Header};

    #[test]
    fn control_frame_round_trip() {
        let frame = Frame {
            seq_id: 0,
            log_id: 42,
            service: 0,
            method: 0,
            headers: vec![Header {
                key: "type".into(),
                value: "reply".into(),
            }],
            payload_encoding: String::new(),
            payload_type: String::new(),
            payload: Vec::new(),
            log_id_new: "abc".into(),
        };
        let encoded = frame.encode_to_vec();
        let decoded = Frame::decode(&encoded).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn event_frame_round_trip() {
        let frame = Frame {
            seq_id: 7,
            log_id: 11,
            service: 1,
            method: 1,
            headers: vec![Header {
                key: "type".into(),
                value: "event".into(),
            }],
            payload_encoding: "json".into(),
            payload_type: "application/json".into(),
            payload: br#"{"header":{"event_id":"evt-1"}}"#.to_vec(),
            log_id_new: String::new(),
        };
        let encoded = frame.encode_to_vec();
        let decoded = Frame::decode(&encoded).unwrap();
        assert_eq!(decoded, frame);
    }
}
