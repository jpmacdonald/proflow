#!/usr/bin/env python3
"""Convert Beblia Holy-Bible-XML-Format files to our JSON format.

Usage: python3 tools/convert_bible_xml.py <input.xml> <output.json>

Input:  <bible><testament><book number="N"><chapter number="N"><verse number="N">text
Output: {"Genesis": {"1": {"1": "text", ...}, ...}, ...}
"""

import json
import sys
import xml.etree.ElementTree as ET

BOOK_NAMES = {
    1: "Genesis", 2: "Exodus", 3: "Leviticus", 4: "Numbers", 5: "Deuteronomy",
    6: "Joshua", 7: "Judges", 8: "Ruth", 9: "1 Samuel", 10: "2 Samuel",
    11: "1 Kings", 12: "2 Kings", 13: "1 Chronicles", 14: "2 Chronicles",
    15: "Ezra", 16: "Nehemiah", 17: "Esther", 18: "Job", 19: "Psalms",
    20: "Proverbs", 21: "Ecclesiastes", 22: "Song of Solomon", 23: "Isaiah",
    24: "Jeremiah", 25: "Lamentations", 26: "Ezekiel", 27: "Daniel",
    28: "Hosea", 29: "Joel", 30: "Amos", 31: "Obadiah", 32: "Jonah",
    33: "Micah", 34: "Nahum", 35: "Habakkuk", 36: "Zephaniah", 37: "Haggai",
    38: "Zechariah", 39: "Malachi", 40: "Matthew", 41: "Mark", 42: "Luke",
    43: "John", 44: "Acts", 45: "Romans", 46: "1 Corinthians",
    47: "2 Corinthians", 48: "Galatians", 49: "Ephesians", 50: "Philippians",
    51: "Colossians", 52: "1 Thessalonians", 53: "2 Thessalonians",
    54: "1 Timothy", 55: "2 Timothy", 56: "Titus", 57: "Philemon",
    58: "Hebrews", 59: "James", 60: "1 Peter", 61: "2 Peter", 62: "1 John",
    63: "2 John", 64: "3 John", 65: "Jude", 66: "Revelation",
}


def convert(xml_path: str, json_path: str) -> None:
    tree = ET.parse(xml_path)
    root = tree.getroot()

    bible = {}
    book_counter = 0

    for testament in root.findall("testament"):
        for book in testament.findall("book"):
            book_counter += 1
            book_num = int(book.get("number", book_counter))
            book_name = BOOK_NAMES.get(book_num)
            if not book_name:
                print(f"Warning: unknown book number {book_num}, skipping")
                continue

            chapters = {}
            for chapter in book.findall("chapter"):
                chapter_num = chapter.get("number")
                verses = {}
                for verse in chapter.findall("verse"):
                    verse_num = verse.get("number")
                    text = (verse.text or "").strip()
                    if text:
                        verses[verse_num] = text
                if verses:
                    chapters[chapter_num] = verses

            if chapters:
                bible[book_name] = chapters

    with open(json_path, "w", encoding="utf-8") as f:
        json.dump(bible, f, ensure_ascii=False, indent=None, separators=(",", ":"))

    book_count = len(bible)
    verse_count = sum(
        len(v) for ch in bible.values() for v in ch.values()
    )
    print(f"Converted {book_count} books, {verse_count} verses -> {json_path}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(__doc__.strip())
        sys.exit(1)
    convert(sys.argv[1], sys.argv[2])
