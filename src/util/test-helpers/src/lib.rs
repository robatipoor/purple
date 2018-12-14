/*
  Copyright 2018 The Purple Library Authors
  This file is part of the Purple Library.

  The Purple Library is free software: you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  The Purple Library is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
  GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with the Purple Library. If not, see <http://www.gnu.org/licenses/>.
*/

extern crate rlp;
extern crate tempfile;
extern crate crypto;
extern crate persistence;
extern crate account;
extern crate patricia_trie;
extern crate hex;
extern crate kvdb_rocksdb;

use std::sync::Arc;
use tempfile::tempdir;
use crypto::Hash;
use persistence::{PersistentDb, Codec, BlakeDbHasher};
use account::{Balance, Address};
use patricia_trie::{TrieMut, TrieDBMut};
use kvdb_rocksdb::{Database, DatabaseConfig};

pub fn init_tempdb() -> PersistentDb {
    let config = DatabaseConfig::with_columns(None);
    let dir = tempdir().unwrap();
    let db = Database::open(&config, dir.path().to_str().unwrap()).unwrap();
    let db_ref = Arc::new(db);

    PersistentDb::new(db_ref, None)
}

pub fn qs<E: Ord>(arr: &mut [E]) {
    if 1 < arr.len() {
        let (mut pivot, mut hi) = (0, arr.len()-1);
        for _ in 0..arr.len()-1 {
            if arr[pivot] < arr[pivot+1] {
                arr.swap(pivot+1, hi);
                hi -= 1;
            } else {
                arr.swap(pivot, pivot+1);
                pivot += 1;
            }
        }
        qs(&mut arr[..pivot]);
        qs(&mut arr[pivot+1..]);
    }
}

pub fn init_balance(
    trie: &mut TrieDBMut<BlakeDbHasher, Codec>,
    address: Address,
    currency_hash: Hash,
    amount: &[u8]
) {
    let bin_address = address.to_bytes();
    let bin_cur_hash = currency_hash.to_vec();

    let hex_address = hex::encode(&bin_address);
    let hex_cur_hash = hex::encode(&bin_cur_hash);

    let cur_key = format!("{}.{}", hex_address, hex_cur_hash);
    let nonce_key = format!("{}.n", hex_address);
    let precision_key = format!("{}.p", hex_cur_hash);

    // Re-serialize balance to validate with regex
    let balance = Balance::from_bytes(amount).unwrap().to_bytes();

    if let Ok(None) = trie.get(b"ci") {
        trie.insert(b"ci", &[0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        trie.insert(b"c.0", &rlp::encode_list::<Vec<u8>, _>(&vec![bin_cur_hash])).unwrap();
    }

    if let Ok(None) = trie.get(&precision_key.as_bytes()) {
        trie.insert(&precision_key.as_bytes(), &[18]).unwrap();
    }
    
    trie.insert(&cur_key.as_bytes(), &balance).unwrap();
    trie.insert(&nonce_key.as_bytes(), &[0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
    trie.commit();
}