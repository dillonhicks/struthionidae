pub(crate) mod deps {
    pub(crate) use failure;
    #[cfg(feature = "generic_array")]
    pub(crate) use generic_array1 as generic_array;
    pub(crate) use parking_lot;
    pub(crate) use tracing;
    pub(crate) use tracing_subscriber;
}

pub mod sync {
    pub use crate::deps::parking_lot::*;
    pub use std::sync::{atomic, Arc, Weak};
}

pub mod result {
    pub type Error = crate::deps::failure::Error;
    pub type Result<T> = std::result::Result<T, Error>;
}

pub mod logging {
    pub use crate::deps::tracing::{debug, error, event, info, span, trace, warn, Level};

    static INTI_LOGGER_ONCE: crate::sync::Once = crate::sync::Once::new();

    pub fn initialize() {
        use crate::deps::tracing_subscriber::{EnvFilter, FmtSubscriber};
        INTI_LOGGER_ONCE.call_once(|| {
            let subscriber = FmtSubscriber::builder()
                .with_env_filter(EnvFilter::from_default_env())
                .finish();
            crate::deps::tracing::subscriber::set_global_default(subscriber).unwrap();
        });
    }
}

pub mod bytes {
    const UPPER_HEX_BYTES: [&'static str; 256] = [
        "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "0A", "0B", "0C", "0D", "0E", "0F", "10", "11",
        "12", "13", "14", "15", "16", "17", "18", "19", "1A", "1B", "1C", "1D", "1E", "1F", "20", "21", "22", "23",
        "24", "25", "26", "27", "28", "29", "2A", "2B", "2C", "2D", "2E", "2F", "30", "31", "32", "33", "34", "35",
        "36", "37", "38", "39", "3A", "3B", "3C", "3D", "3E", "3F", "40", "41", "42", "43", "44", "45", "46", "47",
        "48", "49", "4A", "4B", "4C", "4D", "4E", "4F", "50", "51", "52", "53", "54", "55", "56", "57", "58", "59",
        "5A", "5B", "5C", "5D", "5E", "5F", "60", "61", "62", "63", "64", "65", "66", "67", "68", "69", "6A", "6B",
        "6C", "6D", "6E", "6F", "70", "71", "72", "73", "74", "75", "76", "77", "78", "79", "7A", "7B", "7C", "7D",
        "7E", "7F", "80", "81", "82", "83", "84", "85", "86", "87", "88", "89", "8A", "8B", "8C", "8D", "8E", "8F",
        "90", "91", "92", "93", "94", "95", "96", "97", "98", "99", "9A", "9B", "9C", "9D", "9E", "9F", "A0", "A1",
        "A2", "A3", "A4", "A5", "A6", "A7", "A8", "A9", "AA", "AB", "AC", "AD", "AE", "AF", "B0", "B1", "B2", "B3",
        "B4", "B5", "B6", "B7", "B8", "B9", "BA", "BB", "BC", "BD", "BE", "BF", "C0", "C1", "C2", "C3", "C4", "C5",
        "C6", "C7", "C8", "C9", "CA", "CB", "CC", "CD", "CE", "CF", "D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7",
        "D8", "D9", "DA", "DB", "DC", "DD", "DE", "DF", "E0", "E1", "E2", "E3", "E4", "E5", "E6", "E7", "E8", "E9",
        "EA", "EB", "EC", "ED", "EE", "EF", "F0", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "FA", "FB",
        "FC", "FD", "FE", "FF",
    ];
    const LOWER_HEX_BYTES: [&'static str; 256] = [
        "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "0a", "0b", "0c", "0d", "0e", "0f", "10", "11",
        "12", "13", "14", "15", "16", "17", "18", "19", "1a", "1b", "1c", "1d", "1e", "1f", "20", "21", "22", "23",
        "24", "25", "26", "27", "28", "29", "2a", "2b", "2c", "2d", "2e", "2f", "30", "31", "32", "33", "34", "35",
        "36", "37", "38", "39", "3a", "3b", "3c", "3d", "3e", "3f", "40", "41", "42", "43", "44", "45", "46", "47",
        "48", "49", "4a", "4b", "4c", "4d", "4e", "4f", "50", "51", "52", "53", "54", "55", "56", "57", "58", "59",
        "5a", "5b", "5c", "5d", "5e", "5f", "60", "61", "62", "63", "64", "65", "66", "67", "68", "69", "6a", "6b",
        "6c", "6d", "6e", "6f", "70", "71", "72", "73", "74", "75", "76", "77", "78", "79", "7a", "7b", "7c", "7d",
        "7e", "7f", "80", "81", "82", "83", "84", "85", "86", "87", "88", "89", "8a", "8b", "8c", "8d", "8e", "8f",
        "90", "91", "92", "93", "94", "95", "96", "97", "98", "99", "9a", "9b", "9c", "9d", "9e", "9f", "a0", "a1",
        "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "aa", "ab", "ac", "ad", "ae", "af", "b0", "b1", "b2", "b3",
        "b4", "b5", "b6", "b7", "b8", "b9", "ba", "bb", "bc", "bd", "be", "bf", "c0", "c1", "c2", "c3", "c4", "c5",
        "c6", "c7", "c8", "c9", "ca", "cb", "cc", "cd", "ce", "cf", "d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7",
        "d8", "d9", "da", "db", "dc", "dd", "de", "df", "e0", "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "e9",
        "ea", "eb", "ec", "ed", "ee", "ef", "f0", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "fa", "fb",
        "fc", "fd", "fe", "ff",
    ];
    pub const HEX_CHARS_PER_BYTE: usize = 2;
    pub const PAGE_SIZE: usize = 4096;
    pub const CHUNK_SIZE_FAST: usize = PAGE_SIZE / HEX_CHARS_PER_BYTE;

    pub struct ChunkIter<'a, T> {
        slice:      &'a [T],
        pos:        usize,
        chunk_size: usize,
    }

    impl<'a, T> Iterator for ChunkIter<'a, T> {
        type Item = &'a [T];

        fn next(&mut self) -> Option<Self::Item> {
            if self.pos >= self.slice.len() {
                None
            } else {
                let next_pos = self.pos + self.chunk_size;
                let end = std::cmp::min(next_pos, self.slice.len());
                let chunk = &self.slice[self.pos..end];
                self.pos = next_pos;
                Some(chunk)
            }
        }
    }

    #[derive(Debug)]
    pub struct HexStr<'a>(&'a [u8]);

    impl<'a> HexStr<'a> {
        pub fn iter_chunks(
            &'a self,
            chunk_size: usize,
        ) -> ChunkIter<'a, u8> {
            ChunkIter {
                slice: &self.0[..],
                pos: 0,
                chunk_size,
            }
        }

        pub fn to_string(&self) -> String {
            crate::bytes::to_lower_hex_string(self.0)
        }
    }

    impl<'a> std::convert::From<&'a [u8]> for HexStr<'a> {
        fn from(bytes: &'a [u8]) -> Self {
            HexStr(bytes)
        }
    }

    impl<'a> std::fmt::Display for HexStr<'a> {
        fn fmt(
            &self,
            f: &mut std::fmt::Formatter,
        ) -> std::fmt::Result {
            <HexStr<'a> as std::fmt::LowerHex>::fmt(self, f)
        }
    }

    impl<'a> std::fmt::LowerHex for HexStr<'a> {
        fn fmt(
            &self,
            f: &mut std::fmt::Formatter,
        ) -> std::fmt::Result {
            for chunk in self.iter_chunks(CHUNK_SIZE_FAST) {
                f.write_str(&to_lower_hex_string(chunk))?;
            }

            Ok(())
        }
    }

    impl<'a> std::fmt::UpperHex for HexStr<'a> {
        fn fmt(
            &self,
            f: &mut std::fmt::Formatter,
        ) -> std::fmt::Result {
            for byte in self.0.iter() {
                write!(f, "{}", to_upper_hex(*byte))?;
            }
            Ok(())
        }
    }

    #[inline]
    pub fn to_upper_hex(byte: u8) -> &'static str {
        let ptr = &UPPER_HEX_BYTES as *const &'static str;
        let idx_ptr = unsafe { ptr.add(byte as usize) };
        unsafe { *idx_ptr }
    }

    #[inline]
    pub fn to_lower_hex(byte: u8) -> &'static str {
        let ptr = &LOWER_HEX_BYTES as *const &'static str;
        let idx_ptr = unsafe { ptr.add(byte as usize) };
        unsafe { *idx_ptr }
    }

    #[inline]
    pub fn to_lower_hex_string(bytes: &[u8]) -> String {
        let mut string = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            string.push_str(to_lower_hex(*b));
        }
        string
    }
}

#[cfg(feature = "generic_array")]
pub mod byte_arrays {
    use crate::{
        bytes::to_lower_hex,
        deps::generic_array::{functional::FunctionalSequence, ArrayLength, GenericArray},
    };
    use failure::_core::mem::MaybeUninit;

    pub type ByteArray<N> = GenericArray<u8, N>;
    pub type StrArray<N> = GenericArray<&'static str, N>;

    #[inline]
    pub fn to_lower_hex_array<N>(array: &ByteArray<N>) -> StrArray<N>
    where
        N: ArrayLength<u8> + ArrayLength<&'static str>,
    {
        let mut strings = MaybeUninit::<StrArray<N>>::uninit();
        array.iter().enumerate().for_each(|(idx, b)| unsafe {
            let mut ptr = strings.as_mut_ptr() as *mut &'static str;
            let mut slot = ptr.add(idx);
            *slot = crate::bytes::to_lower_hex(*b);
        });

        unsafe { strings.assume_init() }
    }

    #[inline]
    pub fn to_lower_hex_string<N>(array: &ByteArray<N>) -> String
    where
        N: ArrayLength<u8> + ArrayLength<&'static str>,
    {
        to_lower_hex_array(array).join("")
    }

    pub struct HexSlice<'a, N: ArrayLength<u8>>(&'a ByteArray<N>);

    impl<'a, N> std::convert::From<&'a ByteArray<N>> for HexSlice<'a, N>
    where
        N: ArrayLength<u8> + ArrayLength<&'static str>,
    {
        fn from(bytes: &'a ByteArray<N>) -> Self {
            HexSlice(bytes)
        }
    }

    impl<'a, N> std::fmt::LowerHex for HexSlice<'a, N>
    where
        N: ArrayLength<u8> + ArrayLength<&'static str>,
    {
        fn fmt(
            &self,
            f: &mut std::fmt::Formatter,
        ) -> std::fmt::Result {
            for byte in self.0.iter() {
                write!(f, "{}", crate::bytes::to_lower_hex(*byte))?;
            }
            Ok(())
        }
    }

    impl<'a, N> std::fmt::UpperHex for HexSlice<'a, N>
    where
        N: ArrayLength<u8> + ArrayLength<&'static str>,
    {
        fn fmt(
            &self,
            f: &mut std::fmt::Formatter,
        ) -> std::fmt::Result {
            for byte in self.0.iter() {
                write!(f, "{}", crate::bytes::to_upper_hex(*byte))?;
            }
            Ok(())
        }
    }
}
