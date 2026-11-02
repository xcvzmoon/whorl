import unittest

from summarize import jaccard, shingles


class ShingleTests(unittest.TestCase):
    def test_same_function_is_symmetric_and_normalizes_both_sides(self):
        left = shingles('Café, ONE two three', 'word', 3)
        right = shingles('CAFE\u0301 one two three!', 'word', 3)
        self.assertEqual(jaccard(left, right), 1)
        self.assertEqual(jaccard(left, right), jaccard(right, left))

    def test_short_text_uses_equality_without_overlapping_long_text(self):
        short = shingles('STOP', 'word', 3)
        self.assertEqual(jaccard(short, shingles('stop!', 'word', 3)), 1)
        self.assertEqual(jaccard(short, shingles('a stop sign', 'word', 3)), 0)
        self.assertIsNone(jaccard(set(), set()))

    def test_character_shingles_retain_word_boundaries(self):
        self.assertNotEqual(shingles('ab cd', 'character', 3), shingles('a bcd', 'character', 3))


if __name__ == '__main__':
    unittest.main()
