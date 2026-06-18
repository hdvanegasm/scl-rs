#!/bin/bash
#
# Generates self-signed mTLS material for a local scl-rs deployment:
#
#   - a self-signed root CA (rootCA.crt / rootCA.key), and
#   - one leaf certificate + private key per party, signed by that CA:
#     server_cert_p{i}.crt / priv_key_p{i}.pem.
#
# Each leaf is valid for 127.0.0.1 only and is presented in BOTH TLS roles
# (the node's server certificate and its client identity), matching how the
# library dials peers by IP and mutually authenticates them.
#
# Usage: bash gen_self_signed_certs.sh <n_parties>
#
# original reference:
# https://users.rust-lang.org/t/use-tokio-tungstenite-with-rustls-instead-of-native-tls-for-secure-websockets/90130

set -euo pipefail

if [[ $# -ne 1 || ! $1 =~ ^[0-9]+$ || $1 -lt 1 ]]; then
  echo "usage: $0 <n_parties>   (n_parties must be an integer >= 1)" >&2
  exit 1
fi
n_parties=$1

mkdir -p certs
cd certs

# Self-signed root CA used to sign every party's leaf certificate.
openssl req -x509 -sha256 -nodes -days 1825 \
  -subj "/C=FI/CN=scl-rs-root" \
  -newkey rsa:2048 -keyout rootCA.key -out rootCA.crt

# v3 extensions for the leaf certificates: not a CA, usable as both a TLS
# server and a TLS client, and valid for the loopback address the nodes dial.
cat <<'EOF' >leaf.ext
authorityKeyIdentifier = keyid,issuer
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth, clientAuth
subjectAltName = @alt_names

[alt_names]
IP.1 = 127.0.0.1
EOF

for i in $(seq 0 $((n_parties - 1))); do
  # Unencrypted private key + certificate signing request for party i.
  openssl req -newkey rsa:2048 -nodes \
    -subj "/C=FI/CN=party-$i" \
    -keyout "priv_key_p$i.pem" -out "server_cert_p$i.csr"

  # Sign the CSR with the root CA, applying the leaf extensions above.
  openssl x509 -req -sha256 -days 365 \
    -in "server_cert_p$i.csr" \
    -CA rootCA.crt -CAkey rootCA.key -CAcreateserial \
    -extfile leaf.ext \
    -out "server_cert_p$i.crt"

  rm -f "server_cert_p$i.csr"
done

# Remove intermediate artifacts; keep only the keys and certificates.
rm -f leaf.ext rootCA.srl

echo "Wrote rootCA.crt and certificates for $n_parties part(ies) into ./certs"
