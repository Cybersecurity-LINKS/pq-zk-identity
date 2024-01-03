
use std::str::FromStr;

use examples::get_address_with_funds;
use examples::random_stronghold_path;
use examples::MemStorage;
use identity_iota::core::FromJson;
use identity_iota::core::Object;
use identity_iota::core::Url;
use identity_iota::core::json;
use identity_iota::credential::Credential;
use identity_iota::credential::CredentialBuilder;
use identity_iota::credential::FailFast;
use identity_iota::credential::Jpt;
use identity_iota::credential::JptCredentialValidator;
use identity_iota::credential::JptCredentialValidatorUtils;
use identity_iota::credential::JwtCredentialValidationOptions;
use identity_iota::credential::SelectiveDiscosurePresentation;
use identity_iota::credential::Subject;
use identity_iota::did::CoreDID;
use identity_iota::did::DID;
use identity_iota::did::DIDUrl;
use identity_iota::iota::IotaClientExt;
use identity_iota::iota::IotaDocument;
use identity_iota::iota::IotaIdentityClientExt;
use identity_iota::iota::NetworkName;
use identity_iota::resolver::Resolver;
use identity_iota::storage::JwkDocumentExt;
use identity_iota::storage::JwpDocumentExt;
use identity_iota::storage::JwkMemStore;
use identity_iota::storage::JwkStorage;
use identity_iota::storage::JwpOptions;
use identity_iota::storage::KeyIdMemstore;
use identity_iota::storage::KeyIdStorage;
use identity_iota::storage::KeyType;
use identity_iota::storage::Storage;
use identity_iota::verification::jws::JwsAlgorithm;
use identity_iota::verification::MethodScope;
use iota_sdk::client::secret::stronghold::StrongholdSecretManager;
use iota_sdk::client::secret::SecretManager;
use iota_sdk::client::Client;
use iota_sdk::client::Password;
use iota_sdk::types::api::core::response::WhiteFlagResponse;
use iota_sdk::types::block::address::Address;
use iota_sdk::types::block::output::AliasOutput;
use iota_sdk::types::block::output::TokenId;
use jsonprooftoken::jpa::algs::ProofAlgorithm;
use jsonprooftoken::jwp::header::PresentationProtectedHeader;
use jsonprooftoken::jwp::presented::JwpPresentedBuilder;

// The API endpoint of an IOTA node, e.g. Hornet.
const api_endpoint: &str = "http://localhost:14265";
// The faucet endpoint allows requesting funds for testing purposes.
const faucet_endpoint: &str = "http://localhost:8091/api/enqueue";


// const api_endpoint: &str = "https://api.testnet.shimmer.network";
// const faucet_endpoint: &str = "https://faucet.testnet.shimmer.network/api/enqueue";



async fn create_did(client: &Client, secret_manager: &SecretManager, storage: &MemStorage, key_type: KeyType, alg: ProofAlgorithm ) -> anyhow::Result<(Address, IotaDocument, String)> {

  // Get an address with funds for testing.
  let address: Address = get_address_with_funds(&client, &secret_manager, faucet_endpoint).await?;

  // Get the Bech32 human-readable part (HRP) of the network.
  let network_name: NetworkName = client.network_name().await?;
  
  // Create a new DID document with a placeholder DID.
  // The DID will be derived from the Alias Id of the Alias Output after publishing.
  let mut document: IotaDocument = IotaDocument::new(&network_name);


  let fragment = document.generate_method_jwp(
    &storage, 
    key_type, 
    alg, 
    None, 
    MethodScope::VerificationMethod
  ).await?;

  // issuer_document.generate_method(
  //   &storage, 
  //   JwkMemStore::ED25519_KEY_TYPE, 
  //   JwsAlgorithm::EdDSA, 
  //   None, 
  //   MethodScope::VerificationMethod
  // ).await?;

  // Construct an Alias Output containing the DID document, with the wallet address
  // set as both the state controller and governor.
  let alias_output: AliasOutput = client.new_did_output(address, document, None).await?;

  // Publish the Alias Output and get the published DID document.
  let document: IotaDocument = client.publish_did_output(&secret_manager, alias_output).await?;
  println!("Published DID document: {document:#}");

  Ok((address, document, fragment))
}


/// Demonstrates how to create an Anonymous Credential with BBS+.
#[tokio::main]
async fn main() -> anyhow::Result<()> {


  // Create a new client to interact with the IOTA ledger.
  let client: Client = Client::builder()
    .with_primary_node(api_endpoint, None)?
    .finish()
    .await?;


  let mut secret_manager_issuer = SecretManager::Stronghold(StrongholdSecretManager::builder()
  .password(Password::from("secure_password_1".to_owned()))
  .build(random_stronghold_path())?);

  
  let storage_issuer: MemStorage = MemStorage::new(JwkMemStore::new(), KeyIdMemstore::new());

  let (_, issuer_document, fragment_issuer): (Address, IotaDocument, String) = 
  create_did(&client, &mut secret_manager_issuer, &storage_issuer, JwkMemStore::BLS12381SHA256_KEY_TYPE, ProofAlgorithm::BLS12381_SHA256).await?;


  // ===========================================================================
  // Step 2: Issuer creates and signs a Verifiable Credential with BBS algorithm.
  // ===========================================================================

  // Create a credential subject indicating the degree earned by Alice.
  let subject: Subject = Subject::from_json_value(json!({
    "name": "Alice",
    "mainCourses": ["Object-oriented Programming", "Mathematics"],
    "degree": {
      "type": "BachelorDegree",
      "name": "Bachelor of Science and Arts",
    },
    "GPA": "4.0",
  }))?;

  // Build credential using subject above and issuer.
  let credential: Credential = CredentialBuilder::default()
    .id(Url::parse("https://example.edu/credentials/3732")?)
    .issuer(Url::parse(issuer_document.id().as_str())?)
    .type_("UniversityDegreeCredential")
    .subject(subject)
    .build()?;

  let credential_jpt: Jpt = issuer_document
    .create_credential_jpt(
      &credential,
      &storage_issuer,
      &fragment_issuer,
      &JwpOptions::default(),
      None,
    )
    .await?;


  // Validate the credential's proof using the issuer's DID Document, the credential's semantic structure,
  // that the issuance date is not in the future and that the expiration date is not in the past:
  let decoded_jpt = JptCredentialValidator::validate::<_, Object>(
      &credential_jpt,
      &issuer_document,
      &JwtCredentialValidationOptions::default(),
      FailFast::FirstError,
    )
    .unwrap();

  assert_eq!(credential, decoded_jpt.credential);


  // ===========================================================================
  // Step 3: Issuer sends the Verifiable Credential to the holder.
  // ===========================================================================
  println!("Sending credential (as JPT) to the holder: {}\n", credential_jpt.as_str());

  let mut resolver: Resolver<IotaDocument> = Resolver::new();
  resolver.attach_iota_handler(client);

  // Resolve Issuer DID
  let issuer: CoreDID = JptCredentialValidatorUtils::extract_issuer_from_jpt(&credential_jpt).unwrap();
  let issuer_document: IotaDocument = resolver.resolve(&issuer).await?;


  // Holder validate the credential and retrieve the JwpIssued, needed to construct the JwpPresented
  let decoded_credential = JptCredentialValidator::validate::<_, Object>(
    &credential_jpt,
    &issuer_document,
    &JwtCredentialValidationOptions::default(),
    FailFast::FirstError,
  )
  .unwrap();

  let method_id = decoded_credential.decoded_jwp.get_issuer_protected_header().kid().unwrap();

  let mut selective_disclosure_presentation = SelectiveDiscosurePresentation::new(&decoded_credential.decoded_jwp);
  selective_disclosure_presentation.undisclose_subject("mainCourses[1]").unwrap();
  selective_disclosure_presentation.undisclose_subject("degree.name").unwrap();


  // A unique random challenge generated by the requester per presentation can mitigate replay attacks.
  let challenge: &str = "475a7984-1bb5-4c4c-a56f-822bccd46440";

  
  let presentation_jpt: Jpt = issuer_document
    .create_presentation_jpt(
      &mut selective_disclosure_presentation,
      method_id,
      &JwpOptions::default().nonce(challenge)
    )
    .await?;


  // ===========================================================================
  // Step 6: Holder sends a verifiable presentation to the verifier.
  // ===========================================================================
  println!("Sending presentation (as JPT) to the verifier: {}\n", presentation_jpt.as_str());
  

  Ok(())
}
