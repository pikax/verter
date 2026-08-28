# Runtime orchestration

Run `programctl frontier` to derive work from receipts and activation state. Run `programctl packet ID` to produce a token-bounded implementation packet. Runtime receipt/round-handle/external directories are supplied explicitly or default to `.runtime/`; they are not committed authority. Conflict/resource metadata does not block readiness; the maintainer coordinates concurrent ownership. A non-READY dispatch exits nonzero before mutation.
