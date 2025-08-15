#![allow(dead_code)]
use std::{fs::File, io::Read, path::Path};

#[derive(Clone)]
pub struct Wasm {
    blob: Vec<u8>,
    is_gzipped: bool,
}

impl Wasm {
    pub fn bytes(self) -> Vec<u8> {
        self.blob
    }

    pub fn from_bytes<B: Into<Vec<u8>>>(blob: B) -> Self {
        let blob = blob.into();

        let is_gzipped = Self::is_gzipped_blob(&blob);

        Self { blob, is_gzipped }
    }

    pub fn from_file<P: AsRef<Path>>(f: P) -> Wasm {
        let mut wasm_data = Vec::new();
        let mut wasm_file = File::open(&f).unwrap_or_else(|e| {
            panic!(
                "Could not open wasm file: {} - Error: {}",
                f.as_ref().display(),
                e
            )
        });
        wasm_file
            .read_to_end(&mut wasm_data)
            .unwrap_or_else(|e| panic!("{}", e.to_string()));

        Self::from_bytes(wasm_data)
    }

    fn is_gzipped_blob(blob: &[u8]) -> bool {
        (blob.len() > 4)
        // Has magic bytes.
        && (blob[0..2] == [0x1F, 0x8B])
    }

    pub fn modified(self) -> Self {
        assert!(self.is_gzipped, "Cannot modify a non-gzipped wasm blob");

        let blob = self.bytes();

        // wasm_bytes are gzipped and the subslice [4..8]
        // is the little endian representation of a timestamp
        // so we just flip a bit in the timestamp
        let mut new_blob = blob.clone();
        *new_blob.get_mut(7).expect("cannot be empty") ^= 1;
        assert_ne!(blob, new_blob);

        Self {
            blob: new_blob,
            is_gzipped: true,
        }
    }
}
