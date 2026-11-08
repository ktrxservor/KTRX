//! Générateur xorshift64 : déterministe, rapide, suffisant pour de la variété syntaxique.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    pub fn decide(&mut self, pct: u32) -> bool {
        (self.next_u64() % 100) < pct as u64
    }

    pub fn hex(&mut self, n: usize) -> String {
        const H: &[u8] = b"0123456789abcdef";
        (0..n).map(|_| H[self.below(16)] as char).collect()
    }

    pub fn alnum(&mut self, n: usize) -> String {
        const A: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        (0..n).map(|_| A[self.below(A.len())] as char).collect()
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}
