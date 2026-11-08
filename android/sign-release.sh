#!/usr/bin/env bash
# Signs the release APK (ADR-0004 section 1) and checks it before and after.
#   android/sign-release.sh <unsigned.apk> <signed.apk> <version>
# The key comes from the environment, never the command line: PEIL_KEYSTORE is
# the path of the PKCS12 key store, PEIL_KEYSTORE_PASSWORD its password (the
# key's too), and the key's alias is peil. The signed APK must carry the
# certificate pinned in android/release-cert.sha256 (PEIL_CERT_PIN names
# another pin file, for a test key); any other key fails the run, since a
# phone refuses an update signed with a different key.
# Checks: the application ID, the version name and code from <version>, not
# debuggable, the core as the only native library and for arm64 only, the
# core's crate notices present, 16 KB alignment, and APK Signature Scheme v3
# alone (the minimum SDK is 31). Needs ANDROID_SDK_ROOT with build-tools and a
# JDK on PATH or JAVA_HOME.
set -euo pipefail
[ $# -eq 3 ] || { echo "usage: $0 <unsigned.apk> <signed.apk> <version>" >&2; exit 2; }
in=$1 out=$2 version=$3
here="$(cd "$(dirname "$0")" && pwd)"
pin_file="${PEIL_CERT_PIN:-$here/release-cert.sha256}"
: "${PEIL_KEYSTORE:?set PEIL_KEYSTORE to the key store}"
: "${PEIL_KEYSTORE_PASSWORD:?set PEIL_KEYSTORE_PASSWORD}"
[ -f "$pin_file" ] || { echo "no pinned certificate at $pin_file" >&2; exit 1; }
sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
[ -n "$sdk" ] || { echo "set ANDROID_SDK_ROOT to the Android SDK" >&2; exit 1; }
tools="$(ls -d "$sdk"/build-tools/*/ | sort -V | tail -1)"
ext=""
case "$(uname -s)" in Linux | Darwin) ;; *) ext=".bat" ;; esac
apksigner="${tools}apksigner$ext" aapt2="${tools}aapt2" zipalign="${tools}zipalign"

fail() { echo "$*" >&2; exit 1; }

IFS=. read -r major minor patch <<<"$version"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "version $version isn't major.minor.patch"
code=$((major * 10000 + minor * 100 + patch))

# The APK as built, before signing.
badging="$("$aapt2" dump badging "$in" | tr -d '\r')"
line="$(grep '^package:' <<<"$badging")"
grep -q " name='io.github.erickramer42.peil' " <<<"$line" || fail "wrong application ID: $line"
grep -q " versionCode='$code' " <<<"$line" || fail "version code isn't $code: $line"
grep -q " versionName='$version' " <<<"$line" || fail "version name isn't $version: $line"
if grep -q '^application-debuggable' <<<"$badging"; then fail "the APK is debuggable"; fi
# The listing is read whole first: `grep -q` stops reading at its first match,
# and under pipefail the cut-off unzip would fail the check at random.
files="$(unzip -Z1 "$in")"
libs="$(grep '^lib/' <<<"$files" || true)"
[ "$libs" = "lib/arm64-v8a/libsensio.so" ] || fail "native libraries other than the arm64 core: $libs"
grep -qx 'assets/licenses/RUST-CRATES' <<<"$files" || fail "the core's crate notices aren't in the APK"

# Aligned for 16 KB pages, then signed with v3 alone.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
"$zipalign" -P 16 -f 4 "$in" "$work/aligned.apk"
"$apksigner" sign --ks "$PEIL_KEYSTORE" --ks-type PKCS12 --ks-key-alias peil \
    --ks-pass env:PEIL_KEYSTORE_PASSWORD \
    --v1-signing-enabled false --v2-signing-enabled false --v3-signing-enabled true \
    --v4-signing-enabled false --min-sdk-version 31 \
    --out "$work/signed.apk" "$work/aligned.apk"

# The signed APK.
"$zipalign" -c -P 16 4 "$work/signed.apk" || fail "the signed APK isn't aligned for 16 KB pages"
verify="$("$apksigner" verify --verbose --print-certs --min-sdk-version 31 "$work/signed.apk" | tr -d '\r')"
for scheme in "v1 scheme (JAR signing): false" "v2 scheme (APK Signature Scheme v2): false" \
    "v3 scheme (APK Signature Scheme v3): true"; do
    grep -qF "Verified using $scheme" <<<"$verify" || fail "expected 'Verified using $scheme'"
done
# apksigner names a signer "Signer #1" (build-tools 36.0) or "V3.0 Signer:"
# (later versions); either way the APK must carry one certificate. The output
# is printed on failure: it holds certificates, which are public.
cert="$(sed -n -E 's/^(V[0-9.]+ )?Signer.* certificate SHA-256 digest: //p' <<<"$verify" | sort -u)"
if [ -z "$cert" ] || [ "$(wc -l <<<"$cert")" != 1 ]; then
    echo "$verify" >&2
    fail "expected one signing certificate, found: ${cert:-none}"
fi
pin="$(tr -d ': \r\n' <"$pin_file" | tr 'A-F' 'a-f')"
[[ "$pin" =~ ^[0-9a-f]{64}$ ]] || fail "$pin_file doesn't hold a SHA-256 fingerprint"
[ "$cert" = "$pin" ] || fail "signed with certificate $cert, but $pin_file pins $pin"

mv "$work/signed.apk" "$out"
echo "signed $out: version $version ($code), certificate SHA-256 $cert"
