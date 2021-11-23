/// An array of dynamically sized bit arrays.
#[derive(Clone, Debug)]
pub struct BitMatrix {
    entry_size: usize,
    entry_count: usize,
    bits: Vec<u64>,
}

pub struct BitMatrixParams {
    pub bits_per_entry: usize,
    pub entry_count: usize,
}

impl BitMatrix {
    pub fn new(params: BitMatrixParams) -> Self {
        let entry_size = params.bits_per_entry / 64 + 1;
        let entry_count = params.entry_count;
        Self {
            entry_size,
            entry_count,
            bits: vec![0; entry_count * entry_size],
        }
    }

    pub fn entry(&self, idx: usize) -> Entry<'_> {
        let start = idx * self.entry_size;
        Entry(&self.bits[start..start + self.entry_size])
    }

    pub fn entry_mut(&mut self, idx: usize) -> EntryMut<'_> {
        let start = idx * self.entry_size;
        EntryMut(&mut self.bits[start..start + self.entry_size])
    }

    pub fn iter(&self) -> impl '_ + Iterator<Item = Entry<'_>> {
        self.bits.chunks(self.entry_size).map(Entry)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.bits.iter_mut().for_each(|b| *b = 0);
    }

    pub fn clear_and_resize(&mut self, new_bits_per_entry: usize) {
        self.clear();
        let needed_entry_size = new_bits_per_entry / 64 + 1;
        if needed_entry_size > self.entry_size {
            self.entry_size = needed_entry_size;
            self.bits.resize(self.entry_size * self.entry_count, 0);
        }
    }
}

pub trait IterableEntry {
    fn len(&self) -> usize;
    fn get_word(&self, idx: usize) -> u64;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A view into a single entry in a bit matrix.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a>(&'a [u64]);

#[allow(dead_code)] // single entry iter is currently only used in tests
impl<'a> Entry<'a> {
    pub fn iter(self) -> EntryIter<Self> {
        EntryIter::new(self)
    }

    pub fn intersection(self, other: Self) -> EntryIntersection<'a> {
        EntryIntersection(self.0, other.0)
    }

    pub fn has(&self, idx: usize) -> bool {
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        self.0[word_idx] & (1_u64 << bit_idx) != 0
    }
}

impl<'a> IterableEntry for Entry<'a> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn get_word(&self, idx: usize) -> u64 {
        self.0[idx]
    }
}

#[derive(Debug)]
pub struct EntryMut<'a>(&'a mut [u64]);

impl<'a> EntryMut<'a> {
    /// Set the bit at an index.
    ///
    /// # Panics
    /// Panics if the index is outside the entry's range.
    pub fn set(&mut self, idx: usize) {
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        self.0[word_idx] |= 1_u64 << bit_idx;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EntryIntersection<'a>(&'a [u64], &'a [u64]);

impl<'a> EntryIntersection<'a> {
    pub fn iter(self) -> EntryIter<Self> {
        EntryIter::new(self)
    }
}

impl<'a> IterableEntry for EntryIntersection<'a> {
    fn len(&self) -> usize {
        self.0.len().min(self.1.len())
    }

    fn get_word(&self, idx: usize) -> u64 {
        self.0[idx] & self.1[idx]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EntryIter<Mask: IterableEntry> {
    m: Mask,
    word_idx: usize,
    // copy each word into the iterator so we can remove bits from it
    // instead of reading from the original bitset every time
    curr_word: u64,
}

impl<Mask: IterableEntry> EntryIter<Mask> {
    fn new(m: Mask) -> Self {
        let curr_word = m.get_word(0);
        Self {
            m,
            word_idx: 0,
            curr_word,
        }
    }
}

impl<Mask: IterableEntry> Iterator for EntryIter<Mask> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.curr_word != 0 {
                let first_bit_idx = self.curr_word.trailing_zeros();
                self.curr_word ^= 1 << first_bit_idx;
                return Some(self.word_idx * 64 + first_bit_idx as usize);
            }
            self.word_idx += 1;
            if self.word_idx >= self.m.len() {
                return None;
            }
            self.curr_word = self.m.get_word(self.word_idx);
        }
    }
}

//
// tests
//

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bitset(idxs: &[usize]) -> Vec<u64> {
        let mut ret = Vec::new();
        for idx in idxs {
            let word_idx = idx / 64;
            if word_idx >= ret.len() {
                ret.resize(word_idx + 1, 0);
            }
            EntryMut(&mut ret).set(*idx);
        }
        ret
    }

    #[test]
    fn set_iter() {
        let m = make_bitset(&[0, 5, 3, 130, 120]);
        itertools::assert_equal(Entry(&m).iter(), [0, 3, 5, 120, 130].iter().cloned());
    }

    #[test]
    fn set_intersection() {
        let m1 = make_bitset(&[0, 5, 128, 191, 2500]);
        let m2 = make_bitset(&[2, 5, 130, 120, 0, 191, 3000]);
        let isect = Entry(&m1).intersection(Entry(&m2));
        itertools::assert_equal(isect.iter(), [0, 5, 191].iter().cloned());
    }
}
