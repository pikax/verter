#[test]
fn sealed_worker_and_channel_cannot_be_minted_out_of_crate() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/mint_attested_worker.rs");
    cases.compile_fail("tests/ui/mint_validated_channel.rs");
}
