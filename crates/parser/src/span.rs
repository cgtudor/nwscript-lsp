/// A byte range in source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start: start as u32,
            end: end as u32,
        }
    }

    pub fn empty(pos: usize) -> Self {
        Self::new(pos, pos)
    }

    /// Merge two spans into one covering both.
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn text<'s>(&self, source: &'s str) -> &'s str {
        &source[self.start as usize..self.end as usize]
    }

    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Index for converting byte offsets to line/column positions.
/// Lines and columns are 0-based (matching LSP convention).
pub struct LineIndex {
    /// Byte offset of the start of each line.
    line_starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self { line_starts }
    }

    /// Convert a byte offset to (line, col), both 0-based.
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let col = offset - self.line_starts[line];
        (line as u32, col)
    }

    /// Convert (line, col) back to a byte offset.
    pub fn offset(&self, line: u32, col: u32) -> Option<u32> {
        self.line_starts
            .get(line as usize)
            .map(|&start| start + col)
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_basic() {
        let src = "hello\nworld\nfoo";
        let idx = LineIndex::new(src);
        assert_eq!(idx.line_col(0), (0, 0)); // 'h'
        assert_eq!(idx.line_col(5), (0, 5)); // '\n'
        assert_eq!(idx.line_col(6), (1, 0)); // 'w'
        assert_eq!(idx.line_col(12), (2, 0)); // 'f'
    }
}
