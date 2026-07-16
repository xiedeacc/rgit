#!/usr/bin/env bash
# Preload a verified OCSP response for certificates carrying Must-Staple.
set -euo pipefail

certificate="${RGIT_OCSP_CERTIFICATE:?set RGIT_OCSP_CERTIFICATE}"
output="${RGIT_OCSP_OUTPUT:?set RGIT_OCSP_OUTPUT}"
ca_file="${RGIT_OCSP_CA_FILE:-/etc/ssl/certs/ca-certificates.crt}"

for command in openssl csplit install nginx systemctl; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "[rgit-ocsp] required command not found: $command" >&2
        exit 127
    }
done

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

csplit -s -f "$tmp/cert-" "$certificate" '/-----BEGIN CERTIFICATE-----/' '{*}' || true
rm -f "$tmp/cert-00"
leaf="$tmp/cert-01"
issuer="$tmp/cert-02"
if [ ! -s "$leaf" ] || [ ! -s "$issuer" ]; then
    echo "[rgit-ocsp] certificate file must contain leaf and issuer" >&2
    exit 1
fi

uri="$(openssl x509 -in "$leaf" -noout -ocsp_uri)"
if [ -z "$uri" ]; then
    echo "[rgit-ocsp] certificate has no OCSP URI" >&2
    exit 1
fi

openssl ocsp -issuer "$issuer" -cert "$leaf" -url "$uri" -no_nonce \
    -respout "$tmp/response.der" -timeout 30
verification="$(openssl ocsp -respin "$tmp/response.der" -issuer "$issuer" \
    -cert "$leaf" -CAfile "$ca_file" -no_nonce -timeout 30)"
printf '%s\n' "$verification"
printf '%s\n' "$verification" | grep -q ': good$'

install -m 0644 "$tmp/response.der" "${output}.new"
mv -f "${output}.new" "$output"
nginx -t
systemctl reload nginx
echo "[rgit-ocsp] refreshed $output"
