#!/usr/bin/env bash
# Script d'installation MirageIA
# Usage : curl -sSf https://raw.githubusercontent.com/<org>/mirageia/main/install.sh | sh

set -euo pipefail

REPO="<org>/mirageia"
INSTALL_DIR="${MIRAGEIA_INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="$HOME/.mirageia"

# Couleurs
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[info]${NC} $1"; }
warn() { echo -e "${YELLOW}[warn]${NC} $1"; }
error() { echo -e "${RED}[error]${NC} $1"; exit 1; }

# Détecter OS et architecture
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Linux*)  os="linux" ;;
        Darwin*) os="macos" ;;
        MINGW*|MSYS*|CYGWIN*) os="windows" ;;
        *) error "OS non supporté : $(uname -s)" ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64) arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) error "Architecture non supportée : $(uname -m)" ;;
    esac

    echo "${os}-${arch}"
}

# Télécharger le binaire
download() {
    local platform="$1"
    local ext="tar.gz"
    if [ "$platform" = "windows-x86_64" ]; then
        ext="zip"
    fi

    local url="https://github.com/${REPO}/releases/latest/download/mirageia-${platform}.${ext}"

    info "Téléchargement depuis ${url}..."

    local tmp_dir
    tmp_dir=$(mktemp -d)
    local archive="${tmp_dir}/mirageia.${ext}"

    if command -v curl > /dev/null 2>&1; then
        curl -sSfL "$url" -o "$archive" || error "Échec du téléchargement. Vérifiez que la release existe."
    elif command -v wget > /dev/null 2>&1; then
        wget -q "$url" -O "$archive" || error "Échec du téléchargement."
    else
        error "curl ou wget requis."
    fi

    # Extraire
    info "Extraction..."
    mkdir -p "$INSTALL_DIR"

    if [ "$ext" = "zip" ]; then
        unzip -qo "$archive" -d "$tmp_dir"
    else
        tar xzf "$archive" -C "$tmp_dir"
    fi

    # Copier le binaire
    local bin_name="mirageia"
    if [ "$(uname -s)" = "MINGW"* ] || [ "$(uname -s)" = "MSYS"* ]; then
        bin_name="mirageia.exe"
    fi

    cp "${tmp_dir}/${bin_name}" "${INSTALL_DIR}/${bin_name}"
    chmod +x "${INSTALL_DIR}/${bin_name}"

    # Nettoyer
    rm -rf "$tmp_dir"

    echo "${INSTALL_DIR}/${bin_name}"
}

# Configurer
setup() {
    # Créer le répertoire de config
    if [ ! -d "$CONFIG_DIR" ]; then
        info "Création de ${CONFIG_DIR}/"
        mkdir -p "$CONFIG_DIR"
    fi

    # Copier le config exemple si pas de config existante
    if [ ! -f "${CONFIG_DIR}/config.toml" ]; then
        info "Configuration par défaut créée dans ${CONFIG_DIR}/config.toml"
        cat > "${CONFIG_DIR}/config.toml" << 'TOML'
# Configuration MirageIA
# Voir https://github.com/<org>/mirageia pour la documentation

[proxy]
# listen_addr = "127.0.0.1:3100"
# log_level = "info"

[detection]
# whitelist = ["localhost", "127.0.0.1"]
TOML
    fi
}

# Vérifier le PATH
check_path() {
    if ! echo "$PATH" | tr ':' '\n' | grep -q "^${INSTALL_DIR}$"; then
        warn "${INSTALL_DIR} n'est pas dans votre PATH."
        echo ""
        echo "  Ajoutez cette ligne à votre ~/.bashrc ou ~/.zshrc :"
        echo ""
        echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
        echo ""
    fi
}

# Main
main() {
    echo ""
    echo "  ╔══════════════════════════════════════════╗"
    echo "  ║  Installation de MirageIA                ║"
    echo "  ╚══════════════════════════════════════════╝"
    echo ""

    local platform
    platform=$(detect_platform)
    info "Plateforme détectée : ${platform}"

    local bin_path
    bin_path=$(download "$platform")
    info "Binaire installé dans ${bin_path}"

    setup

    # Vérifier l'installation
    if "${bin_path}" --version > /dev/null 2>&1; then
        local version
        version=$("${bin_path}" --version 2>&1)
        info "Installation réussie : ${version}"
    else
        warn "Le binaire est installé mais ne répond pas à --version."
    fi

    check_path

    echo ""
    info "Pour démarrer MirageIA :"
    echo ""
    echo "    mirageia"
    echo ""
    info "Puis configurez votre application :"
    echo ""
    echo "    export ANTHROPIC_BASE_URL=http://localhost:3100"
    echo ""
}

main "$@"
