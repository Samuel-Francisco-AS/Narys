//! Only synthetic data inside the H2 filesystem/DBus namespace. Never personal.
mod security {
    pub mod audit {
        include!("../../../src-tauri/src/security/audit.rs");
    }
    pub mod secrets {
        include!("../../../src-tauri/src/security/secrets.rs");
    }
}

use security::secrets::{SecretKey, SecretStore};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    // No arbitrary vault path or personal mode. Namespace sentinel is synthetic.
    if args.len() != 2
        || !matches!(args[1].as_str(), "initialize" | "presence")
        || std::env::var("HOME").ok().as_deref() != Some("/home/fixture")
        || std::fs::read("/h2/synthetic-only").ok().as_deref() != Some(b"H2_SYNTHETIC_ONLY")
    {
        std::process::exit(2);
    }
    let store = SecretStore::new("/state/vault".into());
    let outcome = if args[1] == "initialize" {
        store.set_secret(SecretKey::Lr3Test, b"public-h2-fixture-value")
    } else {
        store.secret_presence(&[SecretKey::Lr3Test]).and_then(|p| {
            if p.get(&SecretKey::Lr3Test) == Some(&true) {
                Ok(())
            } else {
                Err(security::secrets::SecretError::Store)
            }
        })
    };
    match outcome {
        Ok(()) => println!(
            "{{\"synthetic_backend_operation\":\"PASS\",\"secret_values_returned\":false}}"
        ),
        Err(error) => {
            println!(
                "{{\"synthetic_backend_operation\":\"BLOCKED\",\"code\":\"{}\"}}",
                error.code()
            );
            std::process::exit(1);
        }
    }
}
