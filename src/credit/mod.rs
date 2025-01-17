// ----- standard library imports
// ----- extra library imports
use axum::routing::Router;
use axum::routing::{get, post};
use bitcoin::bip32 as btc32;
use thiserror::Error;
// ----- local modules
pub mod admin;
mod keys;
pub mod mint;
pub mod persistence;
mod web;
// ----- local imports

pub type Result<T> = std::result::Result<T, Error>;

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum Error {
    #[error("Quote expired: {0}")]
    QuoteExpired(uuid::Uuid),
    #[error("Quote not found: {0}")]
    QuoteNotFound(uuid::Uuid),
    #[error("Quote has been already resolved: {0}")]
    QuoteAlreadyResolved(uuid::Uuid),

    /// keyset errors
    #[error("Keyset already exists: {0:?} {1:?}")]
    KeysetAlreadyExists(keys::KeysetID, btc32::DerivationPath),
}
impl axum::response::IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        todo!();
    }
}

#[derive(Clone)]
pub struct Controller {
    pub quote_service: mint::Service<persistence::InMemoryQuoteRepository>,
}

#[allow(dead_code)]
pub fn pub_routes(ctrl: Controller) -> Router {
    let v1_credit = Router::new()
        .route("/mint/quote", post(web::enquire_quote))
        .route("/mint/quote/:id", get(web::lookup_quote));

    Router::new().nest("/credit/v1", v1_credit).with_state(ctrl)
}

#[allow(dead_code)]
pub fn admin_routes(ctrl: Controller) -> Router {
    let admin = Router::new()
        .route("/quote/pending", get(admin::list_pending_quotes))
        .route("/quote/accepted", get(admin::list_accepted_quotes))
        .route("/quote/:id", get(admin::lookup_quote))
        .route("/quote/:id", post(admin::resolve_quote));
    Router::new()
        .nest("/admin/credit/v1", admin)
        .with_state(ctrl)
}
