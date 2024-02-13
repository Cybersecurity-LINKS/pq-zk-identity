use identity_core::common::Object;
use identity_credential::credential::{Credential, Jws, Jwt};
use identity_document::document::CoreDocument;
use identity_verification::jws::{CharSet, CompactJwsEncoder, CompactJwsEncodingOptions, JwsHeader};
use identity_verification::{jws::JwsAlgorithm, MethodScope};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use crate::{JwkStoragePQ, JwsSignatureOptions, KeyIdStorage, KeyType, Storage, StorageResult};
use crate::JwkGenOutput;
use crate::key_id_storage::MethodDigest;
use super::JwkStorageDocumentError as Error;
use identity_did::DIDUrl;
use identity_verification::{MethodData, VerificationMethod};
use crate::try_undo_key_generation;

///New trait to handle JWP-based operations on DID Documents
#[cfg_attr(not(feature = "send-sync-storage"), async_trait(?Send))]
#[cfg_attr(feature = "send-sync-storage", async_trait)]
pub trait JwsDocumentExtPQC {

  /// Generate new key material in the given `storage` and insert a new verification method with the corresponding
  /// public key material into the DID document. This support BBS+ keys.
  async fn generate_method_pqc<K, I>(
    &mut self,
    storage: &Storage<K, I>,
    key_type: KeyType,
    alg: JwsAlgorithm,
    fragment: Option<&str>,
    scope: MethodScope,
  ) -> StorageResult<String>
  where
    K: JwkStoragePQ,
    I: KeyIdStorage;


  /// Create a JWS using a PQC
  async fn create_jws_pqc<K, I>(
    &self,
    storage: &Storage<K, I>,
    fragment: &str,
    payload: &[u8],
    options: &JwsSignatureOptions,
  ) -> StorageResult<Jws>
  where
    K: JwkStoragePQ,
    I: KeyIdStorage;

  /// Produces a JWT with PQC
  async fn create_credential_jwt_pqc<K, I, T>(
    &self,
    credential: &Credential<T>,
    storage: &Storage<K, I>,
    fragment: &str,
    options: &JwsSignatureOptions,
    custom_claims: Option<Object>,
  ) -> StorageResult<Jwt>
  where
    K: JwkStoragePQ,
    I: KeyIdStorage,
    T: ToOwned<Owned = T> + Serialize + DeserializeOwned + Sync;

}




// ====================================================================================================================
// CoreDocument
// ====================================================================================================================


generate_method_for_document_type!(CoreDocument, JwsAlgorithm, JwkStoragePQ, JwkStoragePQ::generate_pq_key, generate_method_core_document);


#[cfg_attr(not(feature = "send-sync-storage"), async_trait(?Send))]
#[cfg_attr(feature = "send-sync-storage", async_trait)]
impl JwsDocumentExtPQC for CoreDocument {
  async fn generate_method_pqc<K, I>(
    &mut self,
    storage: &Storage<K, I>,
    key_type: KeyType,
    alg: JwsAlgorithm,
    fragment: Option<&str>,
    scope: MethodScope,
  ) -> StorageResult<String>
  where
    K: JwkStoragePQ,
    I: KeyIdStorage,
  {
    // todo!()
    generate_method_core_document(self, storage, key_type, alg, fragment, scope).await
  }

  async fn create_jws_pqc<K, I>(
    &self,
    storage: &Storage<K, I>,
    fragment: &str,
    payload: &[u8],
    options: &JwsSignatureOptions,
  ) -> StorageResult<Jws>
  where
    K: JwkStoragePQ,
    I: KeyIdStorage,
  {
    // Obtain the method corresponding to the given fragment.
    let method: &VerificationMethod = self.resolve_method(fragment, None).ok_or(Error::MethodNotFound)?;
    let MethodData::PublicKeyJwk(ref jwk) = method.data() else {
      return Err(Error::NotPublicKeyJwk);
    };

    // Extract JwsAlgorithm.
    let alg: JwsAlgorithm = jwk
      .alg()
      .unwrap_or("")
      .parse()
      .map_err(|_| Error::InvalidJwsAlgorithm)?;

    // Create JWS header in accordance with options.
    let header: JwsHeader = {
      let mut header = JwsHeader::new();

      header.set_alg(alg);
      if let Some(custom) = &options.custom_header_parameters {
        header.set_custom(custom.clone())
      }

      if let Some(ref kid) = options.kid {
        header.set_kid(kid.clone());
      } else {
        header.set_kid(method.id().to_string());
      }

      if options.attach_jwk {
        header.set_jwk(jwk.clone())
      };

      if let Some(b64) = options.b64 {
        // Follow recommendation in https://datatracker.ietf.org/doc/html/rfc7797#section-7.
        if !b64 {
          header.set_b64(b64);
          header.set_crit(["b64"]);
        }
      };

      if let Some(typ) = &options.typ {
        header.set_typ(typ.clone())
      } else {
        // https://www.w3.org/TR/vc-data-model/#jwt-encoding
        header.set_typ("JWT")
      }

      if let Some(cty) = &options.cty {
        header.set_cty(cty.clone())
      };

      if let Some(url) = &options.url {
        header.set_url(url.clone())
      };

      if let Some(nonce) = &options.nonce {
        header.set_nonce(nonce.clone())
      };

      header
    };

    // Get the key identifier corresponding to the given method from the KeyId storage.
    let method_digest: MethodDigest = MethodDigest::new(method).map_err(Error::MethodDigestConstructionError)?;
    let key_id = <I as KeyIdStorage>::get_key_id(storage.key_id_storage(), &method_digest)
      .await
      .map_err(Error::KeyIdStorageError)?;

    // Extract Compact JWS encoding options.
    let encoding_options: CompactJwsEncodingOptions = if !options.detached_payload {
      // We use this as a default and don't provide the extra UrlSafe check for now.
      // Applications that require such checks can easily do so after JWS creation.
      CompactJwsEncodingOptions::NonDetached {
        charset_requirements: CharSet::Default,
      }
    } else {
      CompactJwsEncodingOptions::Detached
    };

    let jws_encoder: CompactJwsEncoder<'_> = CompactJwsEncoder::new_with_options(payload, &header, encoding_options)
      .map_err(|err| Error::EncodingError(err.into()))?;
    let signature = <K as JwkStoragePQ>::pq_sign(storage.key_storage(), &key_id, jws_encoder.signing_input(), jwk)
      .await
      .map_err(Error::KeyStorageError)?;
    Ok(Jws::new(jws_encoder.into_jws(&signature)))
  }

  async fn create_credential_jwt_pqc<K, I, T>(
    &self,
    credential: &Credential<T>,
    storage: &Storage<K, I>,
    fragment: &str,
    options: &JwsSignatureOptions,
    custom_claims: Option<Object>,
  ) -> StorageResult<Jwt>
  where
    K: JwkStoragePQ,
    I: KeyIdStorage,
    T: ToOwned<Owned = T> + Serialize + DeserializeOwned + Sync,
  {
    if options.detached_payload {
      return Err(Error::EncodingError(Box::<dyn std::error::Error + Send + Sync>::from(
        "cannot use detached payload for credential signing",
      )));
    }

    if !options.b64.unwrap_or(true) {
      // JWTs should not have `b64` set per https://datatracker.ietf.org/doc/html/rfc7797#section-7.
      return Err(Error::EncodingError(Box::<dyn std::error::Error + Send + Sync>::from(
        "cannot use `b64 = false` with JWTs",
      )));
    }

    let payload = credential
      .serialize_jwt(custom_claims)
      .map_err(Error::ClaimsSerializationError)?;
    self
      .create_jws_pqc(storage, fragment, payload.as_bytes(), options)
      .await
      .map(|jws| Jwt::new(jws.into()))
  }

}




// ====================================================================================================================
// IotaDocument
// ====================================================================================================================
#[cfg(feature = "iota-document")]
mod iota_document {

use super::*;
  use identity_iota_core::IotaDocument;

  generate_method_for_document_type!(IotaDocument, JwsAlgorithm, JwkStoragePQ, JwkStoragePQ::generate_pq_key, generate_method_iota_document);

  #[cfg_attr(not(feature = "send-sync-storage"), async_trait(?Send))]
  #[cfg_attr(feature = "send-sync-storage", async_trait)]
  impl JwsDocumentExtPQC for IotaDocument {
    async fn generate_method_pqc<K, I>(
      &mut self,
      storage: &Storage<K, I>,
      key_type: KeyType,
      alg: JwsAlgorithm,
      fragment: Option<&str>,
      scope: MethodScope,
    ) -> StorageResult<String>
    where
      K: JwkStoragePQ,
      I: KeyIdStorage,
    {
      generate_method_iota_document(self, storage, key_type, alg, fragment, scope).await
    }

    async fn create_jws_pqc<K, I>(
      &self,
      storage: &Storage<K, I>,
      fragment: &str,
      payload: &[u8],
      options: &JwsSignatureOptions,
    ) -> StorageResult<Jws>
    where
      K: JwkStoragePQ,
      I: KeyIdStorage,
    {
      self
        .core_document()
        .create_jws_pqc(storage, fragment, payload, options)
        .await
    }

    async fn create_credential_jwt_pqc<K, I, T>(
      &self,
      credential: &Credential<T>,
      storage: &Storage<K, I>,
      fragment: &str,
      options: &JwsSignatureOptions,
      custom_claims: Option<Object>,
    ) -> StorageResult<Jwt>
    where
      K: JwkStoragePQ,
      I: KeyIdStorage,
      T: ToOwned<Owned = T> + Serialize + DeserializeOwned + Sync,
    {
      self
        .core_document()
        .create_credential_jwt_pqc(credential, storage, fragment, options, custom_claims)
        .await
    }

  }
}