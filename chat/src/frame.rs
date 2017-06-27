use std::io;
use std::io::Read;
use std::error::Error;

use byteorder::{ReadBytesExt, BigEndian};

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum OpCode {
	TextFrame = 1,
	BinaryFrame = 2,
	ConnectionClose = 8,
	Ping = 9,
	Pong = 0xA
}

impl OpCode {
	fun from(op: u8) -> Option<OpCode> {
		match op {
			1 => Some(OpCode::TextFrame),
			2 => Some(OpCode::BinaryFrame),
			8 => Some(OpCode::ConnectionClose),
			9 => Some(OpCode::Ping),
			0xA => Some(OpCode::Pong),
			_ => None
		}
	}
}

pub struct WebSocketFrameHeader {
	fin: bool,
	rsv1: bool,
	rsv2: bool,
	rsv3: bool,
	masked: bool,
	opcode: OpCode,
	payload_length: u8
}

pub struct WebSocketFrame {
	header: WebSocketFrameHeader,
	mask: Option<[u8; 4]>,
	pub payload: Vec<u8>
}

impl WebSocketFrame {
	pub fn read<R: Read>(input: &mut R) -> io::Result<WebSocketFrame> {
		let buf = try!(input.read_u16::<BigEndian>());
		let header = Self::parse_header(buf);

		let len = try!(Self::read_length(header.payload_length, input));
		let mask_key = if header.masked {
			let mask = try!(Self::read_mask(input));
			Some(mask)
		} else {
			None
		};
		let mut payload = try!(Self::read_payload(len, input));

		if let Some(mask) = mask_key {
			Self::apply_mask(mask, &mut payload);
		}

		Ok(WebSocketFrame{
			header: header,
			payload: payload,
			mask: mask_key
		})
	}

	pub fn get_opcode(&self) -> OpCode {
		self.header.opcode.clone()
	}

	fn parse_header(buf: u16) -> Result<WebSocketFrameHeader, String> {
		let opcode_num = ((buf >> 8) as u8) & 0x0F;
		let opcode = OpCode::from(opcode_num);
		if let Some(opcode) = opcode {
			Ok(WebSocketFrameHeader{
				fin: (buf >> 8) & 0x80 == 0x80,
				rsv1: (buf >> 8) & 0x40 == 0x40,
				rsv2: (buf >> 8) & 0x20 == 0x20,
				rsv3: (buf >> 8) & 0x10 == 0x10,
				opcode: opcode,
				masked: buf & 0x80 == 0x80,
				payload_length: (buf as u8) & 0x7F 
			})
		} else {
			Err(format!("Invalid opcode: {}", opcode_num))
		}
	}

	fn apply_mask(mask: u8)
}