#!/bin/sh
# Installateur KTRX pour Linux et macOS.
#
#   curl -fsSL https://ktrx-site.tiagoantunes2009.workers.dev/install.sh | sh
#   wget -qO- https://ktrx-site.tiagoantunes2009.workers.dev/install.sh | sh
#
# Variables facultatives :
#   KTRX_VERSION=0.5.3        installe cette version au lieu de la dernière
#   KTRX_INSTALL=/chemin      dossier d'installation (défaut : ~/.ktrx ; le binaire va dans bin/)
#   KTRX_NO_MODIFY_PATH=1     ne modifie aucun fichier de configuration du shell
#
# Le binaire téléchargé est vérifié (SHA-256 publié par le registre) avant d'être installé.
# Rien n'est installé ailleurs que dans le dossier d'installation, et aucun droit
# administrateur n'est nécessaire.

set -eu

REGISTRY="${KTRX_REGISTRY:-https://ktrx-site.tiagoantunes2009.workers.dev}"

say() { printf '%s\n' "$*"; }
warn() { printf 'attention : %s\n' "$*" >&2; }
die() {
	printf '\nerreur : %s\n' "$*" >&2
	exit 1
}

has() { command -v "$1" >/dev/null 2>&1; }

# download URL FILE HEADERS -> prints the HTTP status (000 when the server could not be reached)
download() {
	if has curl; then
		curl -sSL --retry 3 --connect-timeout 15 -D "$3" -o "$2" -w '%{http_code}' "$1" 2>/dev/null || true
	elif has wget; then
		wget -q -S -O "$2" "$1" 2>"$3" || true
		code="$(grep -E '^ *HTTP/' "$3" | tail -n 1 | awk '{print $2}')"
		echo "${code:-000}"
	else
		die "ni curl ni wget n'est installé. Installez l'un des deux, puis relancez cette commande."
	fi
}

sha256_of() {
	if has sha256sum; then
		sha256sum "$1" | cut -d ' ' -f 1
	elif has shasum; then
		shasum -a 256 "$1" | cut -d ' ' -f 1
	elif has openssl; then
		openssl dgst -sha256 "$1" | sed 's/.*= *//'
	else
		die "aucun outil SHA-256 (sha256sum, shasum, openssl) : impossible de vérifier le téléchargement."
	fi
}

# Candidate downloads for this machine, best first.
platforms_for_machine() {
	os="$(uname -s)"
	arch="$(uname -m)"
	case "$os" in
	Linux)
		case "$arch" in
		x86_64 | amd64) echo "linux" ;;
		aarch64 | arm64 | armv8*) echo "linux-arm64" ;;
		*) die "processeur non pris en charge : $arch. KTRX existe pour Linux x86_64 et arm64." ;;
		esac
		;;
	Darwin)
		# A Terminal running under Rosetta reports x86_64 on an Apple Silicon Mac.
		if [ "$arch" = "arm64" ] || [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then
			echo "macos"
		else
			# Since 0.5.3 "macos" is a universal binary; older releases had a separate Intel build.
			echo "macos-intel macos"
		fi
		;;
	MINGW* | MSYS* | CYGWIN*)
		die "sous Windows, ouvrez PowerShell et lancez :  irm $REGISTRY/install.ps1 | iex"
		;;
	*)
		die "système non pris en charge : $os. KTRX existe pour Windows, Linux et macOS."
		;;
	esac
}

add_to_path() {
	bin_dir="$1"
	line="export PATH=\"$bin_dir:\$PATH\""
	changed=""
	shell_name="$(basename "${SHELL:-sh}")"

	append_line() { # file
		if [ -f "$1" ] && grep -F "$bin_dir" "$1" >/dev/null 2>&1; then
			return 0
		fi
		mkdir -p "$(dirname "$1")"
		printf '\n# KTRX\n%s\n' "$2" >>"$1"
		changed="$changed $1"
	}

	case "$shell_name" in
	zsh) append_line "${ZDOTDIR:-$HOME}/.zshrc" "$line" ;;
	bash)
		append_line "$HOME/.bashrc" "$line"
		# macOS Terminal starts login shells, which read .bash_profile and not .bashrc.
		[ "$(uname -s)" = "Darwin" ] && append_line "$HOME/.bash_profile" "$line"
		;;
	fish) append_line "$HOME/.config/fish/conf.d/ktrx.fish" "fish_add_path -g \"$bin_dir\"" ;;
	esac
	append_line "$HOME/.profile" "$line"
	[ -n "$changed" ] && say "PATH mis à jour dans :$changed"
	return 0
}

main() {
	[ -n "${HOME:-}" ] || die "la variable HOME n'est pas définie."
	install_root="${KTRX_INSTALL:-$HOME/.ktrx}"
	bin_dir="$install_root/bin"
	query=""
	[ -n "${KTRX_VERSION:-}" ] && query="?version=$KTRX_VERSION"

	say "Installation de KTRX"

	tmp="$(mktemp -d 2>/dev/null || mktemp -d -t ktrx)"
	trap 'rm -rf "$tmp"' EXIT INT TERM

	# Unsupported systems stop here with their own message.
	candidates="$(platforms_for_machine)"
	platform=""
	status=""
	for candidate in $candidates; do
		status="$(download "$REGISTRY/dl/engine/$candidate$query" "$tmp/archive" "$tmp/headers")"
		if [ "$status" = "200" ]; then
			platform="$candidate"
			break
		fi
		[ "$status" = "404" ] || break
	done
	if [ -z "$platform" ]; then
		case "$status" in
		404)
			if [ -n "$query" ]; then
				die "la version ${KTRX_VERSION} de KTRX n'existe pas pour cette machine ($(uname -s) $(uname -m))."
			fi
			die "aucune version de KTRX n'est encore publiée pour cette machine ($(uname -s) $(uname -m))."
			;;
		000 | "") die "impossible de joindre $REGISTRY. Vérifiez la connexion Internet (et HTTPS_PROXY derrière un proxy)." ;;
		*) die "le registre a répondu HTTP $status. Réessayez dans quelques minutes." ;;
		esac
	fi

	expected="$(grep -i 'x-ktrx-sha256' "$tmp/headers" | tail -n 1 | sed 's/.*: *//' | tr -d '\r ' | tr 'A-F' 'a-f')"
	[ -n "$expected" ] || die "le registre n'a pas publié d'empreinte SHA-256 pour ce fichier : installation annulée."
	actual="$(sha256_of "$tmp/archive")"
	[ "$actual" = "$expected" ] || die "le fichier téléchargé est corrompu (SHA-256 inattendu). Relancez l'installation."

	mkdir -p "$tmp/x"
	tar -xzf "$tmp/archive" -C "$tmp/x" || die "archive illisible."
	[ -f "$tmp/x/ktrx" ] || die "l'archive ne contient pas le programme ktrx."

	mkdir -p "$bin_dir" || die "impossible de créer $bin_dir."
	# Replace atomically, so a running ktrx is never left half-written.
	cp "$tmp/x/ktrx" "$bin_dir/.ktrx.new"
	chmod 755 "$bin_dir/.ktrx.new"
	mv -f "$bin_dir/.ktrx.new" "$bin_dir/ktrx"
	if [ "$(uname -s)" = "Darwin" ]; then
		xattr -d com.apple.quarantine "$bin_dir/ktrx" 2>/dev/null || true
	fi

	version="$("$bin_dir/ktrx" --version 2>&1)" || die "ktrx a été installé mais ne démarre pas sur cette machine : $version"

	say "$version installé : $bin_dir/ktrx"

	case ":$PATH:" in
	*":$bin_dir:"*)
		say ""
		say "C'est prêt. Essayez :  ktrx search"
		;;
	*)
		if [ "${KTRX_NO_MODIFY_PATH:-}" = "1" ]; then
			say ""
			say "Ajoutez $bin_dir à votre PATH pour lancer ktrx par son nom."
		else
			add_to_path "$bin_dir"
			say ""
			say "Ouvrez un nouveau terminal, ou lancez d'abord :"
			say "  export PATH=\"$bin_dir:\$PATH\""
			say "puis essayez :  ktrx search"
		fi
		;;
	esac
}

main "$@"
