# Runtime orchestration

Run `programctl frontier` to derive work from receipts and leases. Run `programctl packet ID` to produce a token-bounded implementation packet. Runtime receipt/lease/external directories are supplied explicitly or default to `.runtime/`; they are not committed authority. A non-READY dispatch exits nonzero before mutation.
