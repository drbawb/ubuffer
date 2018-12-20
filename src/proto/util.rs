use crate::error::ProtoError;

use byteorder::{NetworkEndian, WriteBytesExt};
use std::io::Cursor;

pub fn get_next_nonce(nonce: &mut u32, counter: &mut u64) -> Result<Box<[u8]>, ProtoError> {
	let buf = vec![0u8; 12];
	let mut cursor = Cursor::new(buf);

	*counter += 1;
	
	cursor.write_u32::<NetworkEndian>(*nonce)?;
	cursor.write_u64::<NetworkEndian>(*counter)?;

	Ok(cursor.into_inner().into_boxed_slice())
}
