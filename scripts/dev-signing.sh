#!/bin/zsh

# Shared by the installer and Cargo runner. Pin the certificate outside the repo
# so adding another identity to the keychain cannot silently change app identity.
resolve_dev_signing_identity() {
  signing_identity_file="$HOME/Library/Application Support/Grindlewald/dev-signing-identity"
  local available
  available="$(/usr/bin/security find-identity -v -p codesigning | /usr/bin/sed -nE 's/^[[:space:]]*[0-9]+\) ([A-Fa-f0-9]{40}) "Developer ID Application:.*$/\1/p')"
  signing_identity="${GRINDLEWALD_SIGNING_IDENTITY:-}"
  if [[ -z "$signing_identity" && -f "$signing_identity_file" ]]; then
    signing_identity="$(<"$signing_identity_file")"
  fi
  if [[ -z "$signing_identity" ]]; then
    local -a identities
    identities=("${(@f)available}")
    if [[ -z "$available" || ${#identities} -ne 1 ]]; then
      echo "Choose a Developer ID Application certificate: set GRINDLEWALD_SIGNING_IDENTITY to its SHA-1 fingerprint from security find-identity -v -p codesigning." >&2
      return 1
    fi
    signing_identity="$identities[1]"
  fi
  if [[ -z "$signing_identity" ]] || ! /usr/bin/grep -Fxq -- "$signing_identity" <<< "$available"; then
    echo "The selected Developer ID Application certificate is unavailable. Restore it in Keychain or set GRINDLEWALD_SIGNING_IDENTITY to an available certificate fingerprint. Ad-hoc signing is disabled." >&2
    return 1
  fi
}

sign_dev_app() {
  /usr/bin/codesign --force --deep --sign "$signing_identity" --timestamp=none \
    --identifier com.jonahclarsen.grindlewald "$1" || return
  /usr/bin/codesign --verify --deep --strict "$1" || return
  /bin/mkdir -p "${signing_identity_file:h}"
  (umask 077; print -r -- "$signing_identity" > "$signing_identity_file")
}
