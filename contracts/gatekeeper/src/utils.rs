use bech32::{Bech32, Hrp};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{from_json, Binary, Deps};
use ripemd::{Digest as RipDigest, Ripemd160};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::error::{new_generic_error, ContractError};
use interface::gatekeeper::SignatureInfo as InterfaceSignatureInfo;

// Signature verification is done on external airdrop claims.
#[cw_serde]
pub struct SignatureInfo {
    pub claim_msg: Binary,
    pub signature: Binary,
}

impl SignatureInfo {
    // Converts data transfer struct from interface package into the one from this package,
    // which has the utility metods that we need.
    pub fn convert(input: InterfaceSignatureInfo) -> Self {
        Self {
            claim_msg: input.claim_msg,
            signature: input.signature,
        }
    }

    pub fn extract_addr_from_claim_msg(&self) -> Result<String, ContractError> {
        let claim_msg = from_json::<ClaimMsg>(&self.claim_msg)?;

        Ok(claim_msg.address)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ClaimMsg {
    // To provide claiming via ledger, the address is passed in the memo field of a cosmos msg.
    #[serde(rename = "memo")]
    pub address: String,
}

#[cw_serde]
pub struct CosmosSignature {
    pub pub_key: Binary,
    pub signature: Binary,
}
impl CosmosSignature {
    pub fn verify(&self, deps: Deps, claim_msg: &Binary) -> Result<bool, ContractError> {
        let hash = Sha256::digest(claim_msg);

        deps.api
            .secp256k1_verify(
                hash.as_ref(),
                self.signature.as_slice(),
                self.pub_key.as_slice(),
            )
            .map_err(|_| ContractError::VerificationFailed)
    }

    pub fn derive_addr_from_pubkey(&self, hrp: &str) -> Result<String, ContractError> {
        // derive external address for merkle proof check
        let digest = Sha256::digest(self.pub_key.as_slice());
        let slice: &[u8] = digest.as_ref();
        let sha_hash: [u8; 32] = slice.try_into().map_err(|_| ContractError::WrongLength)?;

        let rip_hash = Ripemd160::digest(sha_hash);

        let hrp = Hrp::parse(hrp).map_err(|_| {
            new_generic_error("Couldn't derive address from pubkey, address prefix parsing failed.")
        })?;

        let addr = bech32::encode::<Bech32>(hrp, rip_hash.as_ref())
            .map_err(|_| ContractError::VerificationFailed {})?;

        Ok(addr)
    }
}
