use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogError {
    InvalidId,
    InvalidFact,
    DuplicateResource,
    DuplicateModel,
    DuplicateEffort,
    ConflictingEconomicFacts,
    BillingDomainNotFound,
    DuplicateAllowanceDimension,
    InvalidOrigin,
    DuplicateRuntimeBinding,
    ResourceNotFound,
    ModelCatalogUnknown,
    ModelNotFound,
    EffortSupportUnknown,
    EffortNotSupported,
    LegacyEffortNotRepresentable,
    DuplicateSnapshot,
    ContextMismatch,
    CapacityExceeded,
}

// Exact spelling is retained: case folding/alias inference would break model scope.
// Model names also allow provider namespace syntax (@family/model:version).
macro_rules! id {
    ($name:ident, $bound:expr, $model:expr) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, CatalogError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > $bound
                    || !value.bytes().all(|b| {
                        b.is_ascii_alphanumeric()
                            || matches!(b, b'-' | b'_' | b'.')
                            || ($model && matches!(b, b'/' | b':' | b'@'))
                    })
                {
                    return Err(CatalogError::InvalidId);
                }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = CatalogError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}
impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // No rejected ID/value in diagnostics.
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CatalogError {}

id!(ResourceId, 64, false);
id!(ProviderFamily, 64, false);
id!(AccessPath, 64, false);
id!(BillingDomainId, 64, false);
id!(RuntimeId, 64, false);
id!(ModelId, 128, true);
id!(EffortId, 64, false);
id!(QualityLabel, 64, false);

id!(AllowanceUnitId, 64, false);
id!(AllowanceDimensionId, 64, false);
