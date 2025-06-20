use std::{fs::File, io::Read, path::Path};

#[derive(Clone)]
pub struct Wasm(Vec<u8>);

impl Wasm {
    pub fn bytes(self) -> Vec<u8> {
        self.0
    }

    pub fn from_bytes<B: Into<Vec<u8>>>(bytes: B) -> Self {
        Self(bytes.into())
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

        Wasm::from_bytes(wasm_data)
    }
}
