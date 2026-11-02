// A retained canonical payload pairs its digest with exactly the bytes
// that digest was computed over. An outside constructor that accepts a
// digest and a byte payload separately could install a mismatched pair
// as an identity, so the payload's storage stays private: identities are
// constructed through the encoder (streaming or exact-size), never
// assembled from parts.
use std::sync::Arc;

use verter_identity::canonical::Canonical;
use verter_identity::encoding::CanonicalDigest;

fn main() {
    let bytes: Arc<[u8]> = Arc::from(&b"not-the-hashed-bytes"[..]);
    let digest = CanonicalDigest::of_bytes(b"different-bytes");
    let _forged = Canonical { digest, bytes };
}
