//! Compatibility layer for Solana SDK types
//!
//! This module provides a unified interface for working with `VersionedMessage`
//! and ensures consistent access to message headers and account keys regardless
//! of the message version (Legacy or V0).
//!
//! ## Purpose
//!
//! Solana SDK has two message formats:
//! - Legacy messages (original format)
//! - V0 messages (with address lookup tables)
//!
//! Both formats have different APIs for accessing the same information.
//! This compatibility layer provides a single, consistent API for both.
//!
//! ## Benefits
//!
//! 1. **Type Safety**: Single source of truth for Pubkey and Signature types from solana-sdk 2.3.13
//! 2. **API Consistency**: Unified methods work with both Legacy and V0 messages
//! 3. **Maintainability**: Changes to message handling only need to be made in one place
//! 4. **Future-proof**: Easy to extend when new message versions are added
//!
//! ## Usage
//!
//! ```rust,no_run
//! use solana_sdk::transaction::VersionedTransaction;
//! use crate::compat;
//!
//! fn process_transaction(tx: &VersionedTransaction) {
//!     // Get message header (works for both Legacy and V0)
//!     let header = compat::get_message_header(&tx.message);
//!     
//!     // Get static account keys (works for both Legacy and V0)
//!     let account_keys = compat::get_static_account_keys(&tx.message);
//!     
//!     // Get required signers (works for both Legacy and V0)
//!     let required_signers = compat::get_required_signers(&tx.message);
//!     
//!     println!("Required signatures: {}", header.num_required_signatures);
//!     println!("First signer: {:?}", required_signers.first());
//! }
//! ```

use solana_sdk::{
    message::{MessageHeader, VersionedMessage},
    pubkey::Pubkey,
};

/// Get the message header from a `VersionedMessage`.
///
/// This works uniformly for both Legacy and V0 message formats.
///
/// # Arguments
///
/// * `message` - A reference to a `VersionedMessage` (Legacy or V0)
///
/// # Returns
///
/// A reference to the `MessageHeader` containing:
/// - `num_required_signatures`: Number of signatures required
/// - `num_readonly_signed_accounts`: Number of readonly accounts that require signatures
/// - `num_readonly_unsigned_accounts`: Number of readonly accounts that don't require signatures
///
/// # Example
///
/// ```rust,no_run
/// use solana_sdk::{message::VersionedMessage, transaction::VersionedTransaction};
/// use crate::compat;
///
/// fn check_signer_count(tx: &VersionedTransaction) -> u8 {
///     let header = compat::get_message_header(&tx.message);
///     header.num_required_signatures
/// }
/// ```
#[inline]
#[must_use]
pub fn get_message_header(message: &VersionedMessage) -> &MessageHeader {
    match message {
        VersionedMessage::Legacy(legacy_msg) => &legacy_msg.header,
        VersionedMessage::V0(v0_msg) => &v0_msg.header,
    }
}

/// Get the static account keys from a `VersionedMessage`.
///
/// Static account keys are the account keys that are directly embedded in the message,
/// as opposed to addresses loaded from lookup tables (V0 only).
///
/// # Arguments
///
/// * `message` - A reference to a `VersionedMessage` (Legacy or V0)
///
/// # Returns
///
/// A slice of `Pubkey`s representing the static account keys in the message.
///
/// # Notes
///
/// - For Legacy messages, this returns all account keys
/// - For V0 messages, this returns only the static keys (not including lookup table addresses)
///
/// # Example
///
/// ```rust,no_run
/// use solana_sdk::{message::VersionedMessage, transaction::VersionedTransaction};
/// use crate::compat;
///
/// fn get_first_account(tx: &VersionedTransaction) -> Option<&solana_sdk::pubkey::Pubkey> {
///     let keys = compat::get_static_account_keys(&tx.message);
///     keys.first()
/// }
/// ```
#[inline]
#[must_use]
pub fn get_static_account_keys(message: &VersionedMessage) -> &[Pubkey] {
    match message {
        VersionedMessage::Legacy(legacy_msg) => &legacy_msg.account_keys,
        VersionedMessage::V0(v0_msg) => &v0_msg.account_keys,
    }
}

/// Get the required signers from a `VersionedMessage`.
///
/// Required signers are the account keys that must sign the transaction.
/// They are always the first N accounts in the static account keys list,
/// where N is `header.num_required_signatures`.
///
/// # Arguments
///
/// * `message` - A reference to a `VersionedMessage` (Legacy or V0)
///
/// # Returns
///
/// A slice of `Pubkey`s representing the accounts that must sign this transaction.
/// The slice is guaranteed to have exactly `header.num_required_signatures` elements.
///
/// # Example
///
/// ```rust,no_run
/// use solana_sdk::{message::VersionedMessage, transaction::VersionedTransaction};
/// use crate::compat;
///
/// fn verify_signer(tx: &VersionedTransaction, expected_signer: &solana_sdk::pubkey::Pubkey) -> bool {
///     let signers = compat::get_required_signers(&tx.message);
///     signers.contains(expected_signer)
/// }
/// ```
#[inline]
#[must_use]
pub fn get_required_signers(message: &VersionedMessage) -> &[Pubkey] {
    let header = get_message_header(message);
    let account_keys = get_static_account_keys(message);
    let num_signers = header.num_required_signatures as usize;

    // Required signers are always the first N accounts
    &account_keys[..num_signers.min(account_keys.len())]
}

/// Get the number of required signatures from a `VersionedMessage`.
///
/// This is a convenience function that extracts the `num_required_signatures`
/// field from the message header.
///
/// # Arguments
///
/// * `message` - A reference to a `VersionedMessage` (Legacy or V0)
///
/// # Returns
///
/// The number of signatures required for this transaction as a `u8`.
///
/// # Example
///
/// ```rust,no_run
/// use solana_sdk::transaction::VersionedTransaction;
/// use crate::compat;
///
/// fn is_multisig(tx: &VersionedTransaction) -> bool {
///     compat::get_num_required_signatures(&tx.message) > 1
/// }
/// ```
#[inline]
#[must_use]
pub fn get_num_required_signatures(message: &VersionedMessage) -> u8 {
    get_message_header(message).num_required_signatures
}

/// Get the number of readonly signed accounts from a `VersionedMessage`.
///
/// # Arguments
///
/// * `message` - A reference to a `VersionedMessage` (Legacy or V0)
///
/// # Returns
///
/// The number of readonly accounts that require signatures as a `u8`.
#[inline]
#[must_use]
pub fn get_num_readonly_signed_accounts(message: &VersionedMessage) -> u8 {
    get_message_header(message).num_readonly_signed_accounts
}

/// Get the number of readonly unsigned accounts from a `VersionedMessage`.
///
/// # Arguments
///
/// * `message` - A reference to a `VersionedMessage` (Legacy or V0)
///
/// # Returns
///
/// The number of readonly accounts that don't require signatures as a `u8`.
#[inline]
#[must_use]
pub fn get_num_readonly_unsigned_accounts(message: &VersionedMessage) -> u8 {
    get_message_header(message).num_readonly_unsigned_accounts
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        hash::Hash,
        message::{v0::Message as MessageV0, Message, VersionedMessage},
        pubkey::Pubkey,
        signature::Keypair,
        signer::Signer,
    };
    // TODO(migrate-system-instruction): temporary allow, full migration post-profit
    #[allow(deprecated)]
    use solana_sdk::system_instruction;

    #[test]
    fn test_legacy_message_header() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 1000);
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let versioned_message = VersionedMessage::Legacy(message);

        let header = get_message_header(&versioned_message);
        assert_eq!(header.num_required_signatures, 1);
    }

    #[test]
    fn test_v0_message_header() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 1000);
        let message_v0 =
            MessageV0::try_compile(&payer.pubkey(), &[instruction], &[], Hash::default()).unwrap();
        let versioned_message = VersionedMessage::V0(message_v0);

        let header = get_message_header(&versioned_message);
        assert_eq!(header.num_required_signatures, 1);
    }

    #[test]
    fn test_legacy_static_account_keys() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 1000);
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let versioned_message = VersionedMessage::Legacy(message);

        let keys = get_static_account_keys(&versioned_message);
        assert!(keys.len() >= 2); // At least payer and recipient
        assert_eq!(keys[0], payer.pubkey());
    }

    #[test]
    fn test_v0_static_account_keys() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 1000);
        let message_v0 =
            MessageV0::try_compile(&payer.pubkey(), &[instruction], &[], Hash::default()).unwrap();
        let versioned_message = VersionedMessage::V0(message_v0);

        let keys = get_static_account_keys(&versioned_message);
        assert!(keys.len() >= 2); // At least payer and recipient
        assert_eq!(keys[0], payer.pubkey());
    }

    #[test]
    fn test_legacy_required_signers() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 1000);
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let versioned_message = VersionedMessage::Legacy(message);

        let signers = get_required_signers(&versioned_message);
        assert_eq!(signers.len(), 1);
        assert_eq!(signers[0], payer.pubkey());
    }

    #[test]
    fn test_v0_required_signers() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 1000);
        let message_v0 =
            MessageV0::try_compile(&payer.pubkey(), &[instruction], &[], Hash::default()).unwrap();
        let versioned_message = VersionedMessage::V0(message_v0);

        let signers = get_required_signers(&versioned_message);
        assert_eq!(signers.len(), 1);
        assert_eq!(signers[0], payer.pubkey());
    }

    #[test]
    fn test_num_required_signatures() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 1000);
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let versioned_message = VersionedMessage::Legacy(message);

        assert_eq!(get_num_required_signatures(&versioned_message), 1);
    }

    #[test]
    fn test_multisig_message() {
        let payer = Keypair::new();
        let signer2 = Keypair::new();
        let recipient = Pubkey::new_unique();

        // Create an instruction that requires two signers
        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 1000);
        let mut message = Message::new(&[instruction], Some(&payer.pubkey()));

        // Manually add second signer to demonstrate multisig
        message.account_keys.insert(1, signer2.pubkey());
        message.header.num_required_signatures = 2;

        let versioned_message = VersionedMessage::Legacy(message);

        let signers = get_required_signers(&versioned_message);
        assert_eq!(signers.len(), 2);
        assert_eq!(get_num_required_signatures(&versioned_message), 2);
    }
}
