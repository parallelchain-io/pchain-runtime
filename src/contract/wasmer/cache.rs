/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Provides a cache for Wasm modules essential for smart contract execution.
//!
//! The `Cache` is a key component used by [SmartContractContext](crate::contract::SmartContractContext),
//! facilitating efficient retrieval of modules compiled from smart contract bytecode.
//! In addition to caching, it maintains critical metadata, including the CBI version and
//! the size of the Wasm bytecode pre-compilation.

use anyhow::Result;
use pchain_types::cryptography::PublicAddress;
use std::{
    io::{Error, ErrorKind, Read, Write},
    path::PathBuf,
    sync::{Arc, RwLock},
};
use wasmer::{DeserializeError, Module, SerializeError};
use wasmer_cache::{Cache as WasmerCache, FileSystemCache};

use crate::contract;

/// Represents the backing storage for Wasm module cache.
/// The `Cache` struct encapsulates a [FileSystemCache] from Wasmer,
/// housed in a directory pointed to by the (`inner`) field.
#[derive(Clone)]
pub struct Cache {
    inner: Arc<RwLock<FileStorage>>,
}

impl Cache {
    /// Instantiate Smart Contract Cache.
    /// # Panics
    /// Will panic the directory failed to construct FileSystemCache.
    pub fn new<P: Into<PathBuf>>(binaries_dir: P) -> Self {
        let sc_path_buf: PathBuf = binaries_dir.into();
        let metadata_path = sc_path_buf.join("metadata");
        let fs_cache = FileSystemCache::new(sc_path_buf).unwrap();
        if !metadata_path.exists() {
            std::fs::create_dir(&metadata_path).unwrap();
        }
        Self {
            inner: Arc::new(RwLock::new(FileStorage {
                metadata_path,
                fs_cache,
            })),
        }
    }

    /// load the cached Module with Metadata from file storage
    pub(crate) fn load(
        &self,
        address: PublicAddress,
        store: &wasmer::Store,
    ) -> Result<(Module, ModuleMetadata), DeserializeError> {
        let key = wasmer_cache::Hash::new(address);
        let file_storage = self
            .inner
            .try_read()
            .map_err(|_| DeserializeError::Io(Error::from(ErrorKind::Interrupted)))?;

        let module = unsafe { file_storage.load(store, key)? };
        let metadata = file_storage
            .metadata(key)
            .map_err(|_| DeserializeError::Io(Error::from(ErrorKind::NotFound)))?;

        Ok((module, metadata))
    }

    /// save the Module with Metadata to file storage
    pub(crate) fn store(
        &self,
        address: PublicAddress,
        module: &wasmer::Module,
        bytes_length: usize,
    ) -> Result<(), SerializeError> {
        let key = wasmer_cache::Hash::new(address);
        let mut file_storage = self
            .inner
            .try_write()
            .map_err(|_| SerializeError::Io(Error::from(ErrorKind::Interrupted)))?;

        file_storage.store(key, module)?;
        file_storage
            .set_metadata(
                key,
                ModuleMetadata {
                    cbi_version: contract::CBI_VERSION,
                    bytecode_length: bytes_length,
                },
            )
            .map_err(|_| SerializeError::Io(Error::from(ErrorKind::NotFound)))?;

        Ok(())
    }
}

/// FileStorage defines the way to store pre-compile contract module
pub(crate) struct FileStorage {
    /// Path to file system to store metadata
    metadata_path: PathBuf,
    /// File system cache for storing pre-compile contract module
    fs_cache: FileSystemCache,
}

impl FileStorage {
    fn metadata(&self, key: wasmer_cache::Hash) -> Result<ModuleMetadata, ()> {
        let path = self.metadata_path.join(key.to_string());
        let mut file = std::fs::File::open(path).map_err(|_| ())?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|_| ())?;
        Ok(ModuleMetadata::from(buf))
    }

    fn set_metadata(
        &mut self,
        key: wasmer_cache::Hash,
        metadata: ModuleMetadata,
    ) -> Result<(), ()> {
        let path = self.metadata_path.join(key.to_string());
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(path)
            .map_err(|e| {
                println!("{:?}", e);
            })?;
        let bytes: Vec<u8> = metadata.into();
        file.write_all(&bytes).map_err(|_| ())?;
        Ok(())
    }
}

impl WasmerCache for FileStorage {
    type SerializeError = SerializeError;
    type DeserializeError = DeserializeError;

    unsafe fn load(
        &self,
        store: &wasmer::Store,
        key: wasmer_cache::Hash,
    ) -> Result<Module, Self::DeserializeError> {
        self.fs_cache.load(store, key)
    }

    fn store(
        &mut self,
        key: wasmer_cache::Hash,
        module: &Module,
    ) -> Result<(), Self::SerializeError> {
        self.fs_cache.store(key, module)
    }
}

/// ModuleMetadata defines the descriptive information about the contract stored in the FileSystemCache.
pub struct ModuleMetadata {
    pub cbi_version: u32,
    pub bytecode_length: usize,
}

impl From<ModuleMetadata> for Vec<u8> {
    fn from(value: ModuleMetadata) -> Self {
        [
            value.cbi_version.to_le_bytes().to_vec(),
            value.bytecode_length.to_le_bytes().to_vec(),
        ]
        .concat()
    }
}

impl From<Vec<u8>> for ModuleMetadata {
    fn from(bytes: Vec<u8>) -> Self {
        let (cbi_bytes, wasm_bytes) = bytes.split_at(std::mem::size_of::<u32>());
        let cbi_version: u32 = u32::from_le_bytes(cbi_bytes.try_into().unwrap());
        let bytecode_length: usize = usize::from_le_bytes(wasm_bytes.try_into().unwrap());
        Self {
            cbi_version,
            bytecode_length,
        }
    }
}
