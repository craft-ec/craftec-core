fn main() {
    #[cfg(feature = "sp1")]
    {
        sp1_build::build_program("../distribution-guest");
    }
}
