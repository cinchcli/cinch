use crate::exit::ExitError;
use client_core::store::Store;

pub fn serve_stdio(_store: &Store) -> Result<(), ExitError> {
    Ok(())
}
