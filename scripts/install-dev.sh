#!/bin/sh
# Installe ktrx-dev (l'outil de l'équipe KTRX) depuis la dernière release GitHub, déjà compilé.
#
#   curl -fsSL https://raw.githubusercontent.com/ktrxservor/KTRX/main/scripts/install-dev.sh | sh
#
# Variables facultatives : KTRX_DEV_VERSION=0.6.0 (sinon la dernière), KTRX_INSTALL (défaut ~/.ktrx).
# Le binaire est vérifié avec SHA256SUMS.txt de la release, installé dans ~/.ktrx/bin, sans droits
# administrateur. Relancer la commande met à jour.

set -eu

REPO="${KTRX_GITHUB_REPO:-ktrxservor/KTRX}"

die() {
	printf '\nerreur : %s\n' "$*" >&2
	exit 1
}
has() { command -v "$1" >/dev/null 2>&1; }

fetch() { # URL FILE
	if has curl; then
		curl -fsSL --retry 3 -o "$2" "$1"
	elif has wget; then
		wget -q -O "$2" "$1"
	else
		die "ni curl ni wget n'est installé."
	fi
}

sha256_of() {
	if has sha256sum; then
		sha256sum "$1" | cut -d ' ' -f 1
	elif has shasum; then
		shasum -a 256 "$1" | cut -d ' ' -f 1
	else
		die "aucun outil SHA-256 (sha256sum ou shasum)."
	fi
}

main() {
	[ -n "${HOME:-}" ] || die "HOME n'est pas défini."
	case "$(uname -s)" in
	Linux)
		case "$(uname -m)" in
		x86_64 | amd64) platform=linux ;;
		aarch64 | arm64) platform=linux-arm64 ;;
		*) die "processeur non pris en charge : $(uname -m)" ;;
		esac
		;;
	Darwin) platform=macos ;;
	*) die "sous Windows, lancez dans PowerShell : irm https://raw.githubusercontent.com/$REPO/main/scripts/install-dev.ps1 | iex" ;;
	esac

	if [ -n "${KTRX_DEV_VERSION:-}" ]; then
		base="https://github.com/$REPO/releases/download/v${KTRX_DEV_VERSION#v}"
	else
		base="https://github.com/$REPO/releases/latest/download"
	fi
	file="ktrx-dev-$platform.tar.gz"
	bin_dir="${KTRX_INSTALL:-$HOME/.ktrx}/bin"

	tmp="$(mktemp -d 2>/dev/null || mktemp -d -t ktrx)"
	trap 'rm -rf "$tmp"' EXIT INT TERM

	echo "Installation de ktrx-dev ($platform)"
	fetch "$base/SHA256SUMS.txt" "$tmp/sums" || die "release introuvable ($base)."
	fetch "$base/$file" "$tmp/$file" || die "$file absent de la release."
	expected="$(grep " \*\{0,1\}$file\$" "$tmp/sums" | cut -d ' ' -f 1)"
	[ -n "$expected" ] || die "$file absent de SHA256SUMS.txt."
	[ "$(sha256_of "$tmp/$file")" = "$expected" ] || die "empreinte SHA-256 inattendue : téléchargement corrompu."

	tar -xzf "$tmp/$file" -C "$tmp"
	mkdir -p "$bin_dir"
	cp "$tmp/ktrx-dev" "$bin_dir/.ktrx-dev.new"
	chmod 755 "$bin_dir/.ktrx-dev.new"
	mv -f "$bin_dir/.ktrx-dev.new" "$bin_dir/ktrx-dev"
	[ "$(uname -s)" = Darwin ] && xattr -d com.apple.quarantine "$bin_dir/ktrx-dev" 2>/dev/null || true

	echo "$("$bin_dir/ktrx-dev" --version) installé : $bin_dir/ktrx-dev"
	case ":$PATH:" in
	*":$bin_dir:"*) ;;
	*)
		line="export PATH=\"$bin_dir:\$PATH\""
		for rc in "$HOME/.profile" "${ZDOTDIR:-$HOME}/.zshrc" "$HOME/.bashrc"; do
			case "$rc" in
			*zshrc) [ "$(basename "${SHELL:-}")" = zsh ] || continue ;;
			*bashrc) [ "$(basename "${SHELL:-}")" = bash ] || continue ;;
			esac
			grep -F "$bin_dir" "$rc" >/dev/null 2>&1 || printf '\n# KTRX\n%s\n' "$line" >>"$rc"
		done
		echo "Ouvrez un nouveau terminal (ou : $line)."
		;;
	esac
	echo "Ensuite : ktrx-dev login, puis ktrx-dev doctor."
}

main "$@"
