//! GF(2^8) arithmetic with irreducible polynomial x^8 + x^4 + x^3 + x^2 + 1 (0x11D)

const POLYNOMIAL: u16 = 0x11D;

/// Log and exp tables for GF(2^8) multiplication
struct Tables {
    log: [u8; 256],
    exp: [u8; 256],
}

static TABLES: std::sync::LazyLock<Tables> = std::sync::LazyLock::new(|| {
    let mut log = [0u8; 256];
    let mut exp = [0u8; 256];

    let mut val: u16 = 1;
    for i in 0..255u16 {
        exp[i as usize] = val as u8;
        log[val as usize] = i as u8;
        val <<= 1;
        if val & 0x100 != 0 {
            val ^= POLYNOMIAL;
        }
    }
    // exp[255] is not used for multiplication but set for completeness
    exp[255] = exp[0];

    Tables { log, exp }
});

/// GF(2^8) field operations
pub struct GF256;

impl GF256 {
    /// Addition in GF(2^8) = XOR
    #[inline]
    pub fn add(a: u8, b: u8) -> u8 {
        a ^ b
    }

    /// Subtraction in GF(2^8) = XOR (same as addition)
    #[inline]
    pub fn sub(a: u8, b: u8) -> u8 {
        a ^ b
    }

    /// Multiplication in GF(2^8) via log/exp tables
    #[inline]
    pub fn mul(a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            return 0;
        }
        let tables = &*TABLES;
        let log_sum = (tables.log[a as usize] as u16 + tables.log[b as usize] as u16) % 255;
        tables.exp[log_sum as usize]
    }

    /// Build a 256-entry lookup table for multiplication by a fixed scalar.
    /// `table[x] = a * x` in GF(2^8). Avoids per-byte log/exp lookups in hot loops.
    #[inline]
    pub fn mul_table(a: u8) -> [u8; 256] {
        let mut table = [0u8; 256];
        if a == 0 {
            return table;
        }
        let tables = &*TABLES;
        let log_a = tables.log[a as usize] as u16;
        for x in 1..=255u8 {
            let log_sum = (log_a + tables.log[x as usize] as u16) % 255;
            table[x as usize] = tables.exp[log_sum as usize];
        }
        table
    }

    /// Multiplicative inverse in GF(2^8). Returns None for 0.
    #[inline]
    pub fn inv(a: u8) -> Option<u8> {
        if a == 0 {
            return None;
        }
        let tables = &*TABLES;
        let log_inv = (255 - tables.log[a as usize] as u16) % 255;
        Some(tables.exp[log_inv as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_is_xor() {
        assert_eq!(GF256::add(0x53, 0xCA), 0x53 ^ 0xCA);
        assert_eq!(GF256::add(0, 0xFF), 0xFF);
        assert_eq!(GF256::add(0xFF, 0xFF), 0);
    }

    #[test]
    fn test_mul_identity() {
        for i in 0..=255u8 {
            assert_eq!(GF256::mul(i, 1), i);
            assert_eq!(GF256::mul(1, i), i);
        }
    }

    #[test]
    fn test_mul_zero() {
        for i in 0..=255u8 {
            assert_eq!(GF256::mul(i, 0), 0);
            assert_eq!(GF256::mul(0, i), 0);
        }
    }

    #[test]
    fn test_mul_commutative() {
        for a in 1..=50u8 {
            for b in 1..=50u8 {
                assert_eq!(GF256::mul(a, b), GF256::mul(b, a));
            }
        }
    }

    #[test]
    fn test_inverse() {
        assert_eq!(GF256::inv(0), None);
        for i in 1..=255u8 {
            let inv = GF256::inv(i).unwrap();
            assert_eq!(GF256::mul(i, inv), 1, "inv({}) = {} but product != 1", i, inv);
        }
    }

    #[test]
    fn test_mul_associative() {
        let a = 0x53u8;
        let b = 0xCAu8;
        let c = 0x7Bu8;
        assert_eq!(
            GF256::mul(GF256::mul(a, b), c),
            GF256::mul(a, GF256::mul(b, c))
        );
    }

    #[test]
    fn test_distributive() {
        let a = 0x53u8;
        let b = 0xCAu8;
        let c = 0x7Bu8;
        // a * (b + c) == a*b + a*c
        assert_eq!(
            GF256::mul(a, GF256::add(b, c)),
            GF256::add(GF256::mul(a, b), GF256::mul(a, c))
        );
    }
}
