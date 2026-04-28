use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::fmt;
use std::ops::Deref;

macro_rules! pubkey_wrapper {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Pubkey);

        impl From<Pubkey> for $name {
            fn from(value: Pubkey) -> Self {
                Self(value)
            }
        }

        impl From<$name> for Pubkey {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl AsRef<Pubkey> for $name {
            fn as_ref(&self) -> &Pubkey {
                &self.0
            }
        }

        impl Deref for $name {
            type Target = Pubkey;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl PartialEq<Pubkey> for $name {
            fn eq(&self, other: &Pubkey) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<$name> for Pubkey {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }
    };
}

pubkey_wrapper!(PoolId);
pubkey_wrapper!(BaseMint);
pubkey_wrapper!(BondingCurveKey);

/// Canonical relation between the three domain identities used by Ghost.
///
/// The pool lifecycle is keyed by `pool_id`, snapshot history by `base_mint`,
/// and curve/account truth by `bonding_curve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PoolIdentity {
    pub pool_id: PoolId,
    pub base_mint: BaseMint,
    pub bonding_curve: BondingCurveKey,
}

/// Thread-safe registry used to translate between pool/base_mint/bonding_curve.
#[derive(Default)]
pub struct PoolIdentityRegistry {
    by_pool: DashMap<PoolId, PoolIdentity>,
    pool_by_base_mint: DashMap<BaseMint, PoolId>,
    pool_by_bonding_curve: DashMap<BondingCurveKey, PoolId>,
}

impl PoolIdentityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, identity: PoolIdentity) -> bool {
        self.pool_by_base_mint
            .insert(identity.base_mint, identity.pool_id);
        self.pool_by_bonding_curve
            .insert(identity.bonding_curve, identity.pool_id);
        self.by_pool.insert(identity.pool_id, identity).is_none()
    }

    pub fn remove_by_pool_id(&self, pool_id: &PoolId) -> Option<PoolIdentity> {
        let (_, identity) = self.by_pool.remove(pool_id)?;
        if self
            .pool_by_base_mint
            .get(&identity.base_mint)
            .map(|entry| *entry == identity.pool_id)
            .unwrap_or(false)
        {
            self.pool_by_base_mint.remove(&identity.base_mint);
        }
        if self
            .pool_by_bonding_curve
            .get(&identity.bonding_curve)
            .map(|entry| *entry == identity.pool_id)
            .unwrap_or(false)
        {
            self.pool_by_bonding_curve.remove(&identity.bonding_curve);
        }
        Some(identity)
    }

    pub fn remove_by_pool(&self, pool_id: &Pubkey) -> Option<PoolIdentity> {
        self.remove_by_pool_id(&PoolId::from(*pool_id))
    }

    pub fn get_by_pool_id(&self, pool_id: &PoolId) -> Option<PoolIdentity> {
        self.by_pool.get(pool_id).map(|entry| *entry)
    }

    pub fn get_by_pool(&self, pool_id: &Pubkey) -> Option<PoolIdentity> {
        self.get_by_pool_id(&PoolId::from(*pool_id))
    }

    pub fn get_by_base_mint_key(&self, base_mint: &BaseMint) -> Option<PoolIdentity> {
        let pool_id = *self.pool_by_base_mint.get(base_mint)?;
        self.get_by_pool_id(&pool_id)
    }

    pub fn get_by_base_mint(&self, base_mint: &Pubkey) -> Option<PoolIdentity> {
        self.get_by_base_mint_key(&BaseMint::from(*base_mint))
    }

    pub fn get_by_bonding_curve_key(
        &self,
        bonding_curve: &BondingCurveKey,
    ) -> Option<PoolIdentity> {
        let pool_id = *self.pool_by_bonding_curve.get(bonding_curve)?;
        self.get_by_pool_id(&pool_id)
    }

    pub fn get_by_bonding_curve(&self, bonding_curve: &Pubkey) -> Option<PoolIdentity> {
        self.get_by_bonding_curve_key(&BondingCurveKey::from(*bonding_curve))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_translates_all_keys() {
        let registry = PoolIdentityRegistry::new();
        let identity = PoolIdentity {
            pool_id: PoolId::from(Pubkey::new_unique()),
            base_mint: BaseMint::from(Pubkey::new_unique()),
            bonding_curve: BondingCurveKey::from(Pubkey::new_unique()),
        };

        assert!(registry.register(identity));
        assert_eq!(registry.get_by_pool(&identity.pool_id), Some(identity));
        assert_eq!(
            registry.get_by_base_mint(&identity.base_mint),
            Some(identity)
        );
        assert_eq!(
            registry.get_by_bonding_curve(&identity.bonding_curve),
            Some(identity)
        );
    }

    #[test]
    fn registry_removal_cleans_reverse_indexes() {
        let registry = PoolIdentityRegistry::new();
        let identity = PoolIdentity {
            pool_id: PoolId::from(Pubkey::new_unique()),
            base_mint: BaseMint::from(Pubkey::new_unique()),
            bonding_curve: BondingCurveKey::from(Pubkey::new_unique()),
        };
        registry.register(identity);

        assert_eq!(registry.remove_by_pool(&identity.pool_id), Some(identity));
        assert!(registry.get_by_pool(&identity.pool_id).is_none());
        assert!(registry.get_by_base_mint(&identity.base_mint).is_none());
        assert!(registry
            .get_by_bonding_curve(&identity.bonding_curve)
            .is_none());
    }

    #[test]
    fn typed_wrappers_and_raw_pubkeys_interoperate() {
        let registry = PoolIdentityRegistry::new();
        let identity = PoolIdentity {
            pool_id: PoolId::from(Pubkey::new_unique()),
            base_mint: BaseMint::from(Pubkey::new_unique()),
            bonding_curve: BondingCurveKey::from(Pubkey::new_unique()),
        };
        registry.register(identity);

        assert_eq!(registry.get_by_pool_id(&identity.pool_id), Some(identity));
        assert_eq!(
            registry.get_by_base_mint_key(&identity.base_mint),
            Some(identity)
        );
        assert_eq!(
            registry.get_by_bonding_curve_key(&identity.bonding_curve),
            Some(identity)
        );
        assert_eq!(
            registry.get_by_pool(identity.pool_id.as_ref()),
            Some(identity)
        );
        assert_eq!(
            Pubkey::from(identity.base_mint).to_string(),
            identity.base_mint.to_string()
        );
    }
}
