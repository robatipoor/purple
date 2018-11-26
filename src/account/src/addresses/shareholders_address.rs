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

use crypto::PublicKey;
use rand::Rng;
use quickcheck::Arbitrary;

#[derive(Hash, PartialEq, Eq, Serialize, Deserialize, Clone, Debug)]
pub struct ShareholdersAddress([u8; 32]);

impl ShareholdersAddress {
    pub const ADDR_TYPE: u8 = 3;

    pub fn from_bytes(bin: &[u8]) -> Result<ShareholdersAddress, &'static str> {
        let addr_type = bin[0];
        
        if bin.len() == 33 && addr_type == Self::ADDR_TYPE {
            let mut addr = [0; 32];
            addr.copy_from_slice(&bin);

            Ok(ShareholdersAddress(addr))
        } else if addr_type != Self::ADDR_TYPE {
            Err("Bad address type!")
        } else {
            Err("Bad slice length!")
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result: Vec<u8> = Vec::new();
        let bytes = &&self.0;

        // Push address type
        result.push(Self::ADDR_TYPE);

        for byte in bytes.iter() {
            result.push(*byte);
        }

        result
    }
}


impl Arbitrary for ShareholdersAddress {
    fn arbitrary<G : quickcheck::Gen>(_g: &mut G) -> ShareholdersAddress {
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..32).map(|_| {
            rng.gen_range(1, 255)
        }).collect();

        let mut result = [0; 32];
        result.copy_from_slice(&bytes);

        ShareholdersAddress(result)
    }

    fn shrink(&self) -> Box<Iterator<Item=Self>> {
        Box::new(self.0.to_vec().shrink().map(|p| {
            let mut result = [0; 32];
            result.copy_from_slice(&p);
            
            ShareholdersAddress(result)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    quickcheck! {
        fn serialize_deserialize(tx: ShareholdersAddress) -> bool {
            tx == ShareholdersAddress::from_bytes(&ShareholdersAddress::to_bytes(&tx)).unwrap()
        }
    }
}