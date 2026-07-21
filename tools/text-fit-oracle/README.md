# Native text-fit oracle

This helper is the deliberately small macOS boundary for presentation text
measurement. It reads versioned JSON requests, one per line, from standard
input and writes one JSON response per line to standard output.

It uses AppKit TextKit (`NSTextStorage`, `NSLayoutManager`, and
`NSTextContainer`) to decode the exact final RTF, shape the installed fonts,
wrap against the inset text-box width, and report:

- whether the complete UTF-16 character range fits;
- the unconstrained size required by the text;
- the complete visual line count;
- the UTF-16 range visible within the constrained box;
- the effective scale used by scale-font-down behavior; and
- the actual AppKit fonts and point sizes used for layout.

The helper reproduces fixed containers, canvas-bounded dynamic-height
containers, and font-downscaling with an explicit lower bound. It rejects text
transformations and font-upscaling modes until their ProPresenter semantics are
evidenced. A typed review error is safer than a plausible but incorrect
measurement.

Cargo compiles it automatically on macOS and exposes its exact build-output
path to the Rust client. For an isolated helper check, build it manually with:

```sh
swiftc -O tools/text-fit-oracle/main.swift -o target/proflow-text-fit-oracle
```

The Rust client owns request validation, protocol correlation, and response
postconditions. Keep policy such as maximum lines and minimum readable font
size outside this helper; it reports physical evidence only.
