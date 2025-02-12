use axum::extract::FromRef;
// ----- standard library imports
// ----- extra library imports
use axum::routing::{get, post};
use axum::Router;
// ----- local modules
//mod credit;
mod credit;
mod keys;
mod persistence;
mod swap;
mod utils;
// ----- local imports

type TStamp = chrono::DateTime<chrono::Utc>;

pub type ProdQuoteKeysRepository = persistence::surreal::keysets::QuoteKeysDB;
pub type ProdKeysRepository = persistence::inmemory::KeysetIDEntryMap;
pub type ProdActiveKeysRepository = persistence::inmemory::KeysetIDEntryMapWithActive;
pub type ProdQuoteRepository = persistence::surreal::quotes::DB;

pub type ProdCreditKeysFactory = credit::keys::Factory<ProdQuoteKeysRepository, ProdKeysRepository>;
pub type ProdQuoteFactory = credit::quotes::Factory<ProdQuoteRepository>;
pub type ProdQuotingService = credit::quotes::Service<ProdCreditKeysFactory, ProdQuoteRepository>;

pub type ProdCreditKeysRepository =
    crate::credit::keys::SwapRepository<ProdKeysRepository, ProdActiveKeysRepository>;
pub type ProdProofRepository = persistence::inmemory::ProofMap;
pub type ProdSwapService = swap::Service<ProdCreditKeysRepository, ProdProofRepository>;

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct AppConfig {
    dbs: persistence::surreal::DBConfig,
}

#[derive(Clone, FromRef)]
pub struct AppController {
    quote: ProdQuotingService,
    swap: ProdSwapService,
}

impl AppController {
    pub async fn new(mint_seed: &[u8], cfg: AppConfig) -> Self {
        let AppConfig { dbs, .. } = cfg;
        let persistence::surreal::DBConfig {
            quotes, quoteskeys, ..
        } = dbs;
        let quotes_repository = ProdQuoteRepository::new(quotes)
            .await
            .expect("DB connection to quotes failed");
        let quote_keys_repository = ProdQuoteKeysRepository::new(quoteskeys)
            .await
            .expect("DB connection to quoteskeys failed");
        let endorsed_keys_repository = ProdKeysRepository::default();
        let maturity_keys_repository = ProdKeysRepository::default();
        let keys_factory = ProdCreditKeysFactory::new(
            mint_seed,
            quote_keys_repository,
            maturity_keys_repository.clone(),
        );
        let quotes_factory = ProdQuoteFactory {
            quotes: quotes_repository.clone(),
        };
        let quoting_service = ProdQuotingService {
            keys_gen: keys_factory,
            quotes_gen: quotes_factory,
            quotes: quotes_repository,
        };

        let debit_keys_repository = ProdActiveKeysRepository::default();
        let credit_keys_for_swaps = ProdCreditKeysRepository {
            debit_keys: debit_keys_repository,
            endorsed_keys: endorsed_keys_repository,
            maturity_keys: maturity_keys_repository,
        };
        let proofs_repo = ProdProofRepository::default();
        let swaps = ProdSwapService {
            keys: credit_keys_for_swaps,
            proofs: proofs_repo,
        };
        Self {
            quote: quoting_service,
            swap: swaps,
        }
    }
}
pub fn credit_routes(ctrl: AppController) -> Router {
    Router::new()
        .route("/v1/swap", post(swap::web::swap_tokens))
        .route("/credit/v1/mint/quote", post(credit::web::enquire_quote))
        .route("/credit/v1/mint/quote/:id", get(credit::web::lookup_quote))
        .route(
            "/admin/credit/v1/quote/pending",
            get(credit::admin::list_pending_quotes),
        )
        .route(
            "/admin/credit/v1/quote/accepted",
            get(credit::admin::list_accepted_quotes),
        )
        .route(
            "/admin/credit/v1/quote/:id",
            get(credit::admin::lookup_quote),
        )
        .route(
            "/admin/credit/v1/quote/:id",
            post(credit::admin::resolve_quote),
        )
        .with_state(ctrl)
}
