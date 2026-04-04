//! Mise à jour automatique de MirageIA via GitHub Releases.
//!
//! Stratégie : téléchargement en arrière-plan + swap au prochain démarrage.
//!
//! 1. Au démarrage du proxy, un thread vérifie silencieusement la dernière version
//! 2. Si une nouvelle version est disponible, le binaire est téléchargé dans ~/.mirageia/staging/
//! 3. Au prochain démarrage, le binaire stagé remplace l'exécutable courant
//! 4. `mirageia update` permet une vérification/installation manuelle

use std::fs;
use std::io::Write;
use std::path::PathBuf;

const GITHUB_REPO: &str = "ctardy/mirageia";

/// Version actuelle du binaire (injectée à la compilation).
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Répertoire de staging pour les mises à jour (~/.mirageia/staging/).
fn staging_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".mirageia").join("staging"))
}

/// Chemin du binaire stagé prêt à être swappé.
fn staged_binary_path() -> Option<PathBuf> {
    let bin_name = if cfg!(windows) {
        "mirageia.exe"
    } else {
        "mirageia"
    };
    staging_dir().map(|d| d.join(bin_name))
}

/// Fichier contenant la version du binaire stagé.
fn staged_version_path() -> Option<PathBuf> {
    staging_dir().map(|d| d.join("version"))
}

/// Détecte la plateforme courante pour le nom d'archive GitHub.
fn platform_artifact() -> Option<&'static str> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        Some("mirageia-windows-x86_64.zip")
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        Some("mirageia-macos-aarch64.tar.gz")
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Some("mirageia-linux-x86_64.tar.gz")
    } else {
        None
    }
}

/// Informations sur la dernière release GitHub.
#[derive(Debug)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: String,
    pub download_url: String,
}

/// Résultat de la vérification de mise à jour.
#[derive(Debug)]
pub enum UpdateStatus {
    /// Une nouvelle version est disponible.
    Available(ReleaseInfo),
    /// Déjà à jour.
    UpToDate,
    /// Une mise à jour est déjà téléchargée et prête (staging).
    Staged(String),
    /// Erreur lors de la vérification.
    Error(String),
}

/// Vérifie la dernière version disponible sur GitHub Releases.
pub async fn check_latest_version() -> UpdateStatus {
    let artifact = match platform_artifact() {
        Some(a) => a,
        None => return UpdateStatus::Error("Plateforme non supportée pour la mise à jour automatique".into()),
    };

    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let client = match reqwest::Client::builder().user_agent("mirageia-updater").build() {
        Ok(c) => c,
        Err(e) => return UpdateStatus::Error(format!("Erreur client HTTP : {}", e)),
    };

    let response = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => return UpdateStatus::Error(format!("Impossible de contacter GitHub : {}", e)),
    };

    if !response.status().is_success() {
        return UpdateStatus::Error(format!("GitHub API retourne {}", response.status()));
    }

    let json: serde_json::Value = match response.json().await {
        Ok(j) => j,
        Err(e) => return UpdateStatus::Error(format!("Réponse JSON invalide : {}", e)),
    };

    let tag = match json["tag_name"].as_str() {
        Some(t) => t.to_string(),
        None => return UpdateStatus::Error("Champ tag_name absent de la réponse".into()),
    };

    let remote_version = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    let local_version = current_version();

    if remote_version == local_version {
        // Vérifier si un binaire est déjà stagé
        if let Some(staged_ver) = read_staged_version() {
            if staged_ver != local_version {
                return UpdateStatus::Staged(staged_ver);
            }
        }
        return UpdateStatus::UpToDate;
    }

    // Comparer les versions (semver simplifié)
    if !is_newer(&remote_version, local_version) {
        return UpdateStatus::UpToDate;
    }

    // Chercher l'URL de téléchargement de l'artefact
    let download_url = json["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find_map(|a| {
                let name = a["name"].as_str()?;
                if name == artifact {
                    a["browser_download_url"].as_str().map(|u| u.to_string())
                } else {
                    None
                }
            })
        });

    match download_url {
        Some(url) => UpdateStatus::Available(ReleaseInfo {
            tag,
            version: remote_version,
            download_url: url,
        }),
        None => UpdateStatus::Error(format!("Artefact '{}' non trouvé dans la release {}", artifact, tag)),
    }
}

/// Compare deux versions semver (ex: "0.2.0" > "0.1.0").
fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.')
            .filter_map(|s| s.parse::<u32>().ok())
            .collect()
    };
    let r = parse(remote);
    let l = parse(local);

    for i in 0..r.len().max(l.len()) {
        let rv = r.get(i).copied().unwrap_or(0);
        let lv = l.get(i).copied().unwrap_or(0);
        if rv > lv {
            return true;
        }
        if rv < lv {
            return false;
        }
    }
    false
}

/// Lit la version du binaire actuellement stagé.
fn read_staged_version() -> Option<String> {
    staged_version_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Télécharge la nouvelle version dans le répertoire de staging.
/// Ne remplace pas le binaire courant — le swap se fait au prochain démarrage.
pub async fn download_to_staging(release: &ReleaseInfo) -> Result<(), String> {
    let staging = staging_dir().ok_or("Impossible de déterminer le répertoire de staging")?;
    fs::create_dir_all(&staging).map_err(|e| format!("Erreur création staging : {}", e))?;

    let client = reqwest::Client::builder()
        .user_agent("mirageia-updater")
        .build()
        .map_err(|e| format!("Erreur client HTTP : {}", e))?;

    let response = client
        .get(&release.download_url)
        .send()
        .await
        .map_err(|e| format!("Erreur téléchargement : {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Téléchargement échoué : HTTP {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Erreur lecture réponse : {}", e))?;

    // Extraire le binaire de l'archive
    let binary_data = extract_binary_from_archive(&bytes)?;

    // Écrire le binaire dans staging
    let bin_path = staged_binary_path()
        .ok_or("Impossible de déterminer le chemin du binaire stagé")?;

    let mut file = fs::File::create(&bin_path)
        .map_err(|e| format!("Erreur écriture binaire : {}", e))?;
    file.write_all(&binary_data)
        .map_err(|e| format!("Erreur écriture binaire : {}", e))?;

    // Rendre exécutable sur Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Erreur chmod : {}", e))?;
    }

    // Écrire la version stagée
    let ver_path = staged_version_path()
        .ok_or("Impossible de déterminer le chemin du fichier version")?;
    fs::write(&ver_path, &release.version)
        .map_err(|e| format!("Erreur écriture version : {}", e))?;

    Ok(())
}

/// Extrait le binaire mirageia d'une archive ZIP ou TAR.GZ.
fn extract_binary_from_archive(data: &[u8]) -> Result<Vec<u8>, String> {
    let bin_name = if cfg!(windows) {
        "mirageia.exe"
    } else {
        "mirageia"
    };

    // Tenter ZIP d'abord (Windows)
    if data.len() >= 4 && data[0..4] == [0x50, 0x4B, 0x03, 0x04] {
        return extract_from_zip(data, bin_name);
    }

    // Sinon TAR.GZ (Unix)
    extract_from_targz(data, bin_name)
}

/// Extrait un fichier d'une archive ZIP en mémoire.
fn extract_from_zip(data: &[u8], target_name: &str) -> Result<Vec<u8>, String> {
    use std::io::{Cursor, Read};

    let reader = Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| format!("Erreur ouverture ZIP : {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Erreur lecture ZIP : {}", e))?;
        let name = file.name().to_string();
        if name == target_name || name.ends_with(&format!("/{}", target_name)) {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| format!("Erreur extraction ZIP : {}", e))?;
            return Ok(buf);
        }
    }

    Err(format!("'{}' non trouvé dans l'archive ZIP", target_name))
}

/// Extrait un fichier d'une archive TAR.GZ en mémoire.
fn extract_from_targz(data: &[u8], target_name: &str) -> Result<Vec<u8>, String> {
    use std::io::{Cursor, Read};

    let gz = flate2::read::GzDecoder::new(Cursor::new(data));
    let mut archive = tar::Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| format!("Erreur lecture TAR : {}", e))?
    {
        let mut entry = entry.map_err(|e| format!("Erreur entrée TAR : {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("Erreur chemin TAR : {}", e))?
            .to_string_lossy()
            .to_string();

        if path == target_name || path.ends_with(&format!("/{}", target_name)) {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("Erreur extraction TAR : {}", e))?;
            return Ok(buf);
        }
    }

    Err(format!(
        "'{}' non trouvé dans l'archive TAR.GZ",
        target_name
    ))
}

/// Applique une mise à jour stagée au démarrage.
///
/// Si un binaire est présent dans staging/ avec une version différente de la courante :
/// 1. Renomme l'exécutable courant en `.old`
/// 2. Copie le binaire stagé vers l'emplacement courant
/// 3. Supprime `.old` et nettoie staging/
///
/// Retourne `Some(version)` si la mise à jour a été appliquée, `None` sinon.
pub fn apply_staged_update() -> Option<String> {
    let staged_version = read_staged_version()?;

    if staged_version == current_version() {
        // Même version, nettoyer le staging
        cleanup_staging();
        return None;
    }

    let staged_bin = staged_binary_path()?;
    if !staged_bin.exists() {
        return None;
    }

    let current_exe = std::env::current_exe().ok()?;

    // Sur Windows, on ne peut pas supprimer un exe en cours mais on peut le renommer
    let old_exe = current_exe.with_extension("old");

    // Étape 1 : renommer le binaire courant → .old
    // Ignorer l'erreur si .old existe déjà (reste d'un update précédent)
    let _ = fs::remove_file(&old_exe);
    if fs::rename(&current_exe, &old_exe).is_err() {
        tracing::warn!("Impossible de renommer le binaire courant pour la mise à jour");
        return None;
    }

    // Étape 2 : copier le binaire stagé → emplacement courant
    if fs::copy(&staged_bin, &current_exe).is_err() {
        // Rollback : restaurer l'ancien binaire
        let _ = fs::rename(&old_exe, &current_exe);
        tracing::warn!("Impossible de copier le nouveau binaire, rollback effectué");
        return None;
    }

    // Rendre exécutable sur Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755));
    }

    // Étape 3 : nettoyer
    let _ = fs::remove_file(&old_exe);
    cleanup_staging();

    Some(staged_version)
}

/// Nettoie le répertoire de staging.
fn cleanup_staging() {
    if let Some(dir) = staging_dir() {
        let _ = fs::remove_dir_all(&dir);
    }
}

/// Lance la vérification et le téléchargement en arrière-plan.
/// Ne bloque pas le thread principal — utilisé au démarrage du proxy.
pub fn spawn_background_check() {
    std::thread::spawn(|| {
        // Petit délai pour ne pas impacter le démarrage
        std::thread::sleep(std::time::Duration::from_secs(5));

        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return,
        };

        rt.block_on(async {
            match check_latest_version().await {
                UpdateStatus::Available(release) => {
                    tracing::info!(
                        "Nouvelle version disponible : v{} (actuelle : v{})",
                        release.version,
                        current_version()
                    );
                    match download_to_staging(&release).await {
                        Ok(()) => {
                            tracing::info!(
                                "v{} téléchargée — sera appliquée au prochain démarrage",
                                release.version
                            );
                        }
                        Err(e) => {
                            tracing::debug!("Échec téléchargement mise à jour : {}", e);
                        }
                    }
                }
                UpdateStatus::Staged(ver) => {
                    tracing::info!(
                        "v{} prête — sera appliquée au prochain démarrage",
                        ver
                    );
                }
                UpdateStatus::UpToDate => {
                    tracing::debug!("MirageIA est à jour (v{})", current_version());
                }
                UpdateStatus::Error(e) => {
                    tracing::debug!("Vérification mise à jour : {}", e);
                }
            }
        });
    });
}

/// Exécute la commande `mirageia update` (mode interactif).
/// Avec `--check` : affiche seulement si une mise à jour est disponible.
/// Sans flag : vérifie, télécharge et applique immédiatement.
pub async fn run_update(check_only: bool) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("  MirageIA v{}", current_version());
    eprintln!("  Vérification des mises à jour...");
    eprintln!();

    match check_latest_version().await {
        UpdateStatus::Available(release) => {
            eprintln!(
                "  ✦ Nouvelle version disponible : v{} (actuelle : v{})",
                release.version,
                current_version()
            );

            if check_only {
                eprintln!();
                eprintln!("  Pour mettre à jour : mirageia update");
                return Ok(());
            }

            eprintln!("  Téléchargement...");
            download_to_staging(&release).await.map_err(|e| {
                eprintln!("  ✗ {}", e);
                e
            })?;

            eprintln!("  Application de la mise à jour...");
            match apply_staged_update() {
                Some(ver) => {
                    eprintln!("  ✓ MirageIA mis à jour vers v{}", ver);
                    eprintln!();
                    eprintln!("  Relancez MirageIA pour utiliser la nouvelle version.");
                }
                None => {
                    eprintln!("  ✗ Impossible d'appliquer la mise à jour automatiquement.");
                    eprintln!("    Le binaire est prêt dans ~/.mirageia/staging/");
                    eprintln!("    Il sera appliqué au prochain démarrage.");
                }
            }
        }
        UpdateStatus::Staged(ver) => {
            eprintln!(
                "  ✦ v{} est déjà téléchargée et prête.",
                ver
            );

            if !check_only {
                eprintln!("  Application...");
                match apply_staged_update() {
                    Some(v) => {
                        eprintln!("  ✓ MirageIA mis à jour vers v{}", v);
                        eprintln!();
                        eprintln!("  Relancez MirageIA pour utiliser la nouvelle version.");
                    }
                    None => {
                        eprintln!("  ✗ Impossible d'appliquer la mise à jour.");
                    }
                }
            } else {
                eprintln!();
                eprintln!("  Pour appliquer : mirageia update");
            }
        }
        UpdateStatus::UpToDate => {
            eprintln!("  ✓ MirageIA est à jour (v{})", current_version());
        }
        UpdateStatus::Error(e) => {
            eprintln!("  ✗ Erreur : {}", e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn test_is_newer_different_lengths() {
        assert!(is_newer("0.2.0", "0.1"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1", "0.1.0"));
    }

    #[test]
    fn test_current_version_not_empty() {
        assert!(!current_version().is_empty());
    }

    #[test]
    fn test_platform_artifact_not_none() {
        // Sur n'importe quelle plateforme de CI, on devrait avoir un artefact
        let artifact = platform_artifact();
        // On ne peut pas garantir que c'est Some sur toutes les plateformes de test
        // mais on vérifie que la fonction ne panique pas
        if let Some(name) = artifact {
            assert!(name.starts_with("mirageia-"));
            assert!(name.contains("x86_64") || name.contains("aarch64"));
        }
    }

    #[test]
    fn test_staging_dir_is_under_mirageia() {
        if let Some(dir) = staging_dir() {
            assert!(dir.to_string_lossy().contains(".mirageia"));
            assert!(dir.to_string_lossy().contains("staging"));
        }
    }
}
