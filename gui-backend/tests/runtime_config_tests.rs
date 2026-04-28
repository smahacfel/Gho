//! Tests for runtime configuration updates
//!
//! These tests verify that runtime configuration changes work correctly
//! and are properly propagated through the system.

#[cfg(test)]
mod tests {
    use gui_backend::{AppState, Settings};
    use std::sync::Arc;

    #[test]
    fn test_runtime_config_update_position_size() {
        let state = Arc::new(AppState::new());

        // Get initial settings
        let initial = state.get_settings();
        assert_eq!(initial.position_size_lamports, 100_000_000); // 0.1 SOL

        // Update position size
        let mut new_settings = initial.clone();
        new_settings.position_size_lamports = 500_000_000; // 0.5 SOL
        state.update_settings(new_settings);

        // Verify update
        let updated = state.get_settings();
        assert_eq!(updated.position_size_lamports, 500_000_000);
    }

    #[test]
    fn test_runtime_config_update_jito_tip() {
        let state = Arc::new(AppState::new());

        // Get initial settings
        let initial = state.get_settings();
        assert_eq!(initial.jito_tip_lamports, 10_000); // 0.00001 SOL
        assert!(initial.auto_jito_tip); // Auto mode by default

        // Update to fixed tip mode with custom amount
        let mut new_settings = initial.clone();
        new_settings.jito_tip_lamports = 50_000; // 0.00005 SOL
        new_settings.auto_jito_tip = false; // Fixed mode
        state.update_settings(new_settings);

        // Verify update
        let updated = state.get_settings();
        assert_eq!(updated.jito_tip_lamports, 50_000);
        assert!(!updated.auto_jito_tip);
    }

    #[test]
    fn test_runtime_config_update_slippage() {
        let state = Arc::new(AppState::new());

        // Get initial settings
        let initial = state.get_settings();
        assert_eq!(initial.max_slippage, 0.01); // 1%

        // Update slippage
        let mut new_settings = initial.clone();
        new_settings.max_slippage = 0.05; // 5%
        state.update_settings(new_settings);

        // Verify update
        let updated = state.get_settings();
        assert_eq!(updated.max_slippage, 0.05);
    }

    #[test]
    fn test_runtime_config_multiple_updates() {
        let state = Arc::new(AppState::new());

        // Perform multiple updates
        for i in 1..=5 {
            let mut settings = state.get_settings();
            settings.position_size_lamports = 100_000_000 * i;
            settings.jito_tip_lamports = 10_000 * i;
            state.update_settings(settings);
        }

        // Verify final state
        let final_settings = state.get_settings();
        assert_eq!(final_settings.position_size_lamports, 500_000_000); // 0.5 SOL
        assert_eq!(final_settings.jito_tip_lamports, 50_000); // 0.00005 SOL
    }

    #[test]
    fn test_runtime_config_to_runtime_config_conversion() {
        let settings = Settings {
            position_size_lamports: 200_000_000,
            jito_tip_lamports: 20_000,
            max_slippage: 0.02,
            enable_jito: true,
            auto_jito_tip: false,
        };

        let runtime_config = settings.to_runtime_config();

        assert_eq!(runtime_config.position_size_lamports, 200_000_000);
        assert_eq!(runtime_config.jito_tip_lamports, 20_000);
        assert_eq!(runtime_config.max_slippage, 0.02);
        assert!(runtime_config.enable_jito);
        assert!(!runtime_config.auto_jito_tip);
    }

    #[test]
    fn test_concurrent_config_reads() {
        use std::thread;

        let state = Arc::new(AppState::new());

        // Update settings
        let mut settings = state.get_settings();
        settings.position_size_lamports = 300_000_000;
        state.update_settings(settings);

        // Spawn multiple threads reading config concurrently
        let mut handles = vec![];
        for _ in 0..10 {
            let state_clone = Arc::clone(&state);
            let handle = thread::spawn(move || {
                let settings = state_clone.get_settings();
                assert_eq!(settings.position_size_lamports, 300_000_000);
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_settings_defaults() {
        let settings = Settings::default();

        assert_eq!(settings.position_size_lamports, 100_000_000); // 0.1 SOL
        assert_eq!(settings.jito_tip_lamports, 10_000); // 0.00001 SOL
        assert_eq!(settings.max_slippage, 0.01); // 1%
        assert!(!settings.enable_jito);
        assert!(settings.auto_jito_tip);
    }
}
