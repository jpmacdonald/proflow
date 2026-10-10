import AppKit
import CoreText
import CryptoKit
import Foundation

private let wireProtocolVersion = 5
private let dimensionTolerance: CGFloat = 0.02
private let binarySearchIterations = 32

private struct Request: Decodable {
    let protocolVersion: Int
    let requestId: UInt64
    let rtfHex: String
    let geometry: Geometry
    let scaleBehavior: String
    let minimumScale: Double
    let maximumContainerHeight: Double
    let transform: String
    let verticalAlignment: String
    let requiredFonts: [String]
}

private struct Geometry: Decodable {
    let width: Double
    let height: Double
    let margins: Margins

    var contentWidth: CGFloat {
        CGFloat(width - margins.left - margins.right)
    }

    var contentHeight: CGFloat {
        CGFloat(height - margins.top - margins.bottom)
    }
}

private struct Margins: Decodable {
    let top: Double
    let left: Double
    let bottom: Double
    let right: Double
}

private struct Response: Encodable {
    let protocolVersion: Int
    let requestId: UInt64
    let status: String
    let evidence: Evidence?
    let error: ResponseError?

    static func success(requestId: UInt64, evidence: Evidence) -> Self {
        Self(
            protocolVersion: wireProtocolVersion,
            requestId: requestId,
            status: "ok",
            evidence: evidence,
            error: nil
        )
    }

    static func failure(requestId: UInt64, error: OracleError) -> Self {
        Self(
            protocolVersion: wireProtocolVersion,
            requestId: requestId,
            status: "error",
            evidence: nil,
            error: ResponseError(code: error.code, message: error.message, details: error.details)
        )
    }
}

private struct ResponseError: Encodable {
    let code: String
    let message: String
    let details: [String]
}

private struct Evidence: Encodable {
    let fitsBounds: Bool
    let usedRect: UsedRect
    let lineCount: Int
    let metricStyleRunCount: Int
    let fittedUtf16Range: Utf16Range
    let inputUtf16Length: Int
    let effectiveScale: Double
    let resolvedFonts: [ResolvedFont]
    let nativeLayoutRuntime: NativeLayoutRuntime
}

/// Count contiguous attributed runs that can change glyph selection, advance,
/// line breaking, or baseline geometry. Paint-only attributes such as color
/// are intentionally excluded because they cannot change textbox fit.
private func metricStyleRunCount(in attributed: NSAttributedString) -> Int {
    guard attributed.length > 0 else { return 0 }
    let fullRange = NSRange(location: 0, length: attributed.length)
    let metricKeys: [NSAttributedString.Key] = [
        .font,
        .paragraphStyle,
        .ligature,
        .kern,
        .baselineOffset,
        .obliqueness,
        .expansion,
        .strokeWidth,
        .writingDirection,
        .verticalGlyphForm,
        NSAttributedString.Key("NSSuperscript"),
    ]
    var boundaries = Set([0, attributed.length])
    for key in metricKeys {
        attributed.enumerateAttribute(key, in: fullRange) { _, range, _ in
            boundaries.insert(range.location)
            boundaries.insert(NSMaxRange(range))
        }
    }
    let ordered = boundaries.sorted()
    let visibleCharacters = CharacterSet.whitespacesAndNewlines.inverted
    var previous: [NSAttributedString.Key: Any]?
    var count = 0
    for (start, end) in zip(ordered, ordered.dropFirst()) where end > start {
        let range = NSRange(location: start, length: end - start)
        let content = (attributed.string as NSString).substring(with: range)
        guard content.rangeOfCharacter(from: visibleCharacters) != nil else { continue }
        var signature: [NSAttributedString.Key: Any] = [:]
        for key in metricKeys {
            if let value = attributed.attribute(key, at: start, effectiveRange: nil) {
                signature[key] = value
            }
        }
        if previous == nil || !metricAttributesEqual(previous!, signature) {
            count += 1
        }
        previous = signature
    }
    return count
}

private func metricAttributesEqual(
    _ left: [NSAttributedString.Key: Any],
    _ right: [NSAttributedString.Key: Any]
) -> Bool {
    guard left.count == right.count else { return false }
    for (key, leftValue) in left {
        guard let rightValue = right[key],
              let leftObject = leftValue as? NSObject,
              let rightObject = rightValue as? NSObject,
              leftObject.isEqual(rightObject)
        else { return false }
    }
    return true
}

private struct UsedRect: Encodable {
    let x: Double
    let y: Double
    let width: Double
    let height: Double
}

private struct NativeLayoutRuntime: Encodable {
    let operatingSystem: String
    let appkit: String
    let coreText: String
}

private struct Utf16Range: Encodable {
    let location: Int
    let length: Int
}

private struct ResolvedFont: Encodable, Comparable {
    let postscriptName: String
    let familyName: String
    let pointSize: Double
    let fontProgramPath: String
    let fontProgramSha256: String

    static func < (left: Self, right: Self) -> Bool {
        if left.postscriptName != right.postscriptName {
            return left.postscriptName < right.postscriptName
        }
        if left.familyName != right.familyName {
            return left.familyName < right.familyName
        }
        if left.pointSize != right.pointSize {
            return left.pointSize < right.pointSize
        }
        if left.fontProgramPath != right.fontProgramPath {
            return left.fontProgramPath < right.fontProgramPath
        }
        return left.fontProgramSha256 < right.fontProgramSha256
    }
}

private struct FontProgramIdentity {
    let path: String
    let sha256: String
}

/// Content identities for font programs used by this persistent oracle.
///
/// A PostScript name alone does not prove which installed font bytes AppKit
/// shaped. The descriptor URL is AppKit's authoritative resolved program; its
/// digest makes layout receipts comparable across machines. The exact local
/// path is also returned so Rust can re-hash the same bytes immediately before
/// committing the artifacts that depend on this measurement.
private final class FontProgramDigests {
    private var values: [URL: String] = [:]

    func identity(for font: NSFont) throws -> FontProgramIdentity {
        guard let url = CTFontCopyAttribute(font as CTFont, kCTFontURLAttribute) as? URL,
              url.isFileURL
        else {
            throw OracleError(
                code: "font_program_unavailable",
                message: "resolved font \(font.fontName) has no file URL"
            )
        }
        let path = url.path
        if let existing = values[url] {
            return FontProgramIdentity(path: path, sha256: existing)
        }
        do {
            let data = try Data(contentsOf: url, options: .mappedIfSafe)
            let digest = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            values[url] = digest
            return FontProgramIdentity(path: path, sha256: digest)
        } catch {
            throw OracleError(
                code: "font_program_unavailable",
                message: "could not read resolved font program \(url.path): \(error.localizedDescription)"
            )
        }
    }
}

private struct LayoutEvidence {
    let fitsBounds: Bool
    let usedRect: CGRect
    let lineCount: Int
    let fittedRange: NSRange
    let inputLength: Int
}

private struct OracleError: Error {
    let code: String
    let message: String
    let details: [String]

    init(code: String, message: String, details: [String] = []) {
        self.code = code
        self.message = message
        self.details = details
    }
}

private func validate(_ request: Request) throws {
    guard request.protocolVersion == wireProtocolVersion else {
        throw OracleError(
            code: "protocol_version_mismatch",
            message: "expected protocol version \(wireProtocolVersion), got \(request.protocolVersion)"
        )
    }
    let dimensions = [
        ("width", request.geometry.width),
        ("height", request.geometry.height),
    ]
    for (name, value) in dimensions where !value.isFinite || value <= 0 {
        throw OracleError(code: "invalid_request", message: "\(name) must be finite and positive")
    }
    let margins = [
        ("top", request.geometry.margins.top),
        ("left", request.geometry.margins.left),
        ("bottom", request.geometry.margins.bottom),
        ("right", request.geometry.margins.right),
    ]
    for (name, value) in margins where !value.isFinite || value < 0 {
        throw OracleError(code: "invalid_request", message: "\(name) margin must be finite and nonnegative")
    }
    guard request.geometry.contentWidth > 0, request.geometry.contentHeight > 0 else {
        throw OracleError(code: "invalid_request", message: "margins consume the text box")
    }
    guard request.minimumScale.isFinite,
          request.minimumScale > 0,
          request.minimumScale <= 1
    else {
        throw OracleError(code: "invalid_request", message: "minimum_scale must be within (0, 1]")
    }
    guard request.maximumContainerHeight.isFinite,
          request.maximumContainerHeight >= Double(request.geometry.contentHeight)
    else {
        throw OracleError(
            code: "invalid_request",
            message: "maximum_container_height must be finite and at least the authored content height"
        )
    }
    guard request.transform == "none" else {
        throw OracleError(code: "unsupported_transform", message: request.transform)
    }
    guard ["none", "adjust_container_height", "scale_font_down"].contains(request.scaleBehavior) else {
        throw OracleError(code: "unsupported_scale_behavior", message: request.scaleBehavior)
    }
    guard ["top", "middle", "bottom"].contains(request.verticalAlignment) else {
        throw OracleError(code: "invalid_request", message: "unknown vertical alignment")
    }
    guard !request.requiredFonts.isEmpty else {
        throw OracleError(code: "invalid_request", message: "at least one required font must be declared")
    }
}

private func decodeHex(_ value: String) throws -> Data {
    guard !value.isEmpty, value.count.isMultiple(of: 2) else {
        throw OracleError(code: "invalid_rtf", message: "RTF hex must be nonempty and even-length")
    }
    var bytes: [UInt8] = []
    bytes.reserveCapacity(value.count / 2)
    var index = value.startIndex
    while index < value.endIndex {
        let next = value.index(index, offsetBy: 2)
        guard let byte = UInt8(value[index ..< next], radix: 16) else {
            throw OracleError(code: "invalid_rtf", message: "RTF hex contains a non-hexadecimal byte")
        }
        bytes.append(byte)
        index = next
    }
    return Data(bytes)
}

private func preflightFonts(_ requiredFonts: [String]) throws {
    let fontManager = NSFontManager.shared
    let availableFamilies = Set(fontManager.availableFontFamilies.map { $0.lowercased() })
    let missing = requiredFonts.filter { name in
        let normalized = name.lowercased()
        return !availableFamilies.contains(normalized) && NSFont(name: name, size: 12) == nil
    }
    guard missing.isEmpty else {
        throw OracleError(
            code: "missing_font",
            message: "required fonts are unavailable",
            details: missing.sorted()
        )
    }
}

private func decodeRtf(_ data: Data) throws -> NSAttributedString {
    do {
        let attributed = try NSAttributedString(
            data: data,
            options: [.documentType: NSAttributedString.DocumentType.rtf],
            documentAttributes: nil
        )
        var containsAttachment = false
        attributed.enumerateAttribute(
            .attachment,
            in: NSRange(location: 0, length: attributed.length)
        ) { value, _, stop in
            if value != nil {
                containsAttachment = true
                stop.pointee = true
            }
        }
        guard !containsAttachment else {
            throw OracleError(
                code: "unsupported_rtf_content",
                message: "text attachments are not supported by the fit oracle"
            )
        }
        return attributed
    } catch let error as OracleError {
        throw error
    } catch {
        throw OracleError(code: "invalid_rtf", message: error.localizedDescription)
    }
}

private func scaled(_ source: NSAttributedString, by scale: CGFloat) throws -> NSAttributedString {
    guard scale != 1 else { return source }
    let result = NSMutableAttributedString(attributedString: source)
    let fullRange = NSRange(location: 0, length: result.length)
    var failedFontName: String?

    result.enumerateAttribute(.font, in: fullRange) { value, range, _ in
        guard let font = value as? NSFont else { return }
        guard let scaledFont = NSFont(
            descriptor: font.fontDescriptor,
            size: font.pointSize * scale
        ) else {
            failedFontName = font.fontName
            return
        }
        result.addAttribute(.font, value: scaledFont, range: range)
    }
    if let failedFontName {
        throw OracleError(
            code: "layout_failed",
            message: "could not scale resolved font \(failedFontName)"
        )
    }
    result.enumerateAttribute(.kern, in: fullRange) { value, range, _ in
        guard let kern = value as? NSNumber else { return }
        result.addAttribute(.kern, value: kern.doubleValue * Double(scale), range: range)
    }
    result.enumerateAttribute(.baselineOffset, in: fullRange) { value, range, _ in
        guard let offset = value as? NSNumber else { return }
        result.addAttribute(
            .baselineOffset,
            value: offset.doubleValue * Double(scale),
            range: range
        )
    }
    result.enumerateAttribute(.paragraphStyle, in: fullRange) { value, range, _ in
        guard let paragraph = value as? NSParagraphStyle,
              let mutable = paragraph.mutableCopy() as? NSMutableParagraphStyle
        else { return }
        mutable.minimumLineHeight *= scale
        mutable.maximumLineHeight *= scale
        mutable.lineSpacing *= scale
        mutable.paragraphSpacing *= scale
        mutable.paragraphSpacingBefore *= scale
        mutable.firstLineHeadIndent *= scale
        mutable.headIndent *= scale
        mutable.tailIndent *= scale
        result.addAttribute(.paragraphStyle, value: mutable, range: range)
    }
    return result
}

private func layout(
    _ attributed: NSAttributedString,
    width: CGFloat,
    height: CGFloat,
    maximumContainerHeight: CGFloat,
    adjustsContainerHeight: Bool,
    verticalAlignment: String
) throws -> LayoutEvidence {
    let storage = NSTextStorage(attributedString: attributed)
    let naturalManager = NSLayoutManager()
    let naturalContainer = NSTextContainer(
        containerSize: NSSize(width: width, height: .greatestFiniteMagnitude)
    )
    naturalContainer.lineFragmentPadding = 0
    naturalContainer.lineBreakMode = .byWordWrapping
    naturalManager.addTextContainer(naturalContainer)
    storage.addLayoutManager(naturalManager)
    naturalManager.ensureLayout(for: naturalContainer)

    let fullGlyphRange = naturalManager.glyphRange(for: naturalContainer)
    let naturalUsedRect = naturalManager.usedRect(for: naturalContainer)
    let effectiveHeight = adjustsContainerHeight
        ? min(maximumContainerHeight, max(height, naturalUsedRect.height))
        : height
    let verticalOffset: CGFloat
    switch verticalAlignment {
    case "top":
        verticalOffset = -naturalUsedRect.minY
    case "middle":
        verticalOffset = ((effectiveHeight - naturalUsedRect.height) / 2) - naturalUsedRect.minY
    case "bottom":
        verticalOffset = effectiveHeight - naturalUsedRect.height - naturalUsedRect.minY
    default:
        throw OracleError(
            code: "unsupported_vertical_alignment",
            message: verticalAlignment
        )
    }
    let usedRect = naturalUsedRect.offsetBy(dx: 0, dy: verticalOffset)
    var lineCount = 0
    naturalManager.enumerateLineFragments(forGlyphRange: fullGlyphRange) { _, _, _, _, _ in
        lineCount += 1
    }

    let constrainedStorage = NSTextStorage(attributedString: attributed)
    let constrainedManager = NSLayoutManager()
    let constrainedContainer = NSTextContainer(
        containerSize: NSSize(width: width, height: effectiveHeight)
    )
    constrainedContainer.lineFragmentPadding = 0
    constrainedContainer.lineBreakMode = .byWordWrapping
    constrainedManager.addTextContainer(constrainedContainer)
    constrainedStorage.addLayoutManager(constrainedManager)
    constrainedManager.ensureLayout(for: constrainedContainer)

    let fittedGlyphRange = constrainedManager.glyphRange(for: constrainedContainer)
    let fittedRange = constrainedManager.characterRange(
        forGlyphRange: fittedGlyphRange,
        actualGlyphRange: nil
    )
    let completeRange = fittedRange.location == 0
        && NSMaxRange(fittedRange) == attributed.length
    let withinDimensions = usedRect.minX >= -dimensionTolerance
        && usedRect.minY >= -dimensionTolerance
        && usedRect.maxX <= width + dimensionTolerance
        && usedRect.maxY <= effectiveHeight + dimensionTolerance
    return LayoutEvidence(
        fitsBounds: completeRange && withinDimensions,
        usedRect: usedRect,
        lineCount: lineCount,
        fittedRange: fittedRange,
        inputLength: attributed.length
    )
}

private func nativeLayoutRuntime() throws -> NativeLayoutRuntime {
    let operatingSystemVersion = ProcessInfo.processInfo.operatingSystemVersionString
    guard !operatingSystemVersion.isEmpty else {
        throw OracleError(
            code: "runtime_identity_unavailable",
            message: "operating-system version is unavailable"
        )
    }
    func frameworkVersion(_ identifier: String) throws -> String {
        guard let value = Bundle(identifier: identifier)?
            .object(forInfoDictionaryKey: "CFBundleVersion")
        else {
            throw OracleError(
                code: "runtime_identity_unavailable",
                message: "framework version is unavailable for \(identifier)"
            )
        }
        let version = String(describing: value)
        guard !version.isEmpty else {
            throw OracleError(
                code: "runtime_identity_unavailable",
                message: "framework version is empty for \(identifier)"
            )
        }
        return version
    }
    return NativeLayoutRuntime(
        operatingSystem: operatingSystemVersion,
        appkit: try frameworkVersion("com.apple.AppKit"),
        coreText: try frameworkVersion("com.apple.CoreText")
    )
}

private func resolvedFonts(
    in attributed: NSAttributedString,
    fontPrograms: FontProgramDigests
) throws -> [ResolvedFont] {
    guard attributed.length > 0 else { return [] }
    var fonts = Set<String>()
    var evidence: [ResolvedFont] = []
    var resolutionError: OracleError?
    attributed.enumerateAttribute(
        .font,
        in: NSRange(location: 0, length: attributed.length)
    ) { value, _, stop in
        guard let font = value as? NSFont else { return }
        let key = "\(font.fontName)\u{0}\(font.familyName ?? "")\u{0}\(font.pointSize)"
        guard fonts.insert(key).inserted else { return }
        do {
            let program = try fontPrograms.identity(for: font)
            evidence.append(
                ResolvedFont(
                    postscriptName: font.fontName,
                    familyName: font.familyName ?? font.fontName,
                    pointSize: Double(font.pointSize),
                    fontProgramPath: program.path,
                    fontProgramSha256: program.sha256
                )
            )
        } catch let error as OracleError {
            resolutionError = error
            stop.pointee = true
        } catch {
            resolutionError = OracleError(
                code: "font_program_unavailable",
                message: error.localizedDescription
            )
            stop.pointee = true
        }
    }
    if let resolutionError { throw resolutionError }
    guard !evidence.isEmpty else {
        throw OracleError(code: "layout_failed", message: "visible text has no resolved font")
    }
    return evidence.sorted()
}

private func measure(
    _ request: Request,
    fontPrograms: FontProgramDigests
) throws -> Evidence {
    try validate(request)
    try preflightFonts(request.requiredFonts)
    let attributed = try decodeRtf(decodeHex(request.rtfHex))
    let width = request.geometry.contentWidth
    let height = request.geometry.contentHeight

    var effectiveScale = 1.0
    var measured = try layout(
        attributed,
        width: width,
        height: height,
        maximumContainerHeight: CGFloat(request.maximumContainerHeight),
        adjustsContainerHeight: request.scaleBehavior == "adjust_container_height",
        verticalAlignment: request.verticalAlignment
    )
    if request.scaleBehavior == "scale_font_down", !measured.fitsBounds {
        let minimum = CGFloat(request.minimumScale)
        let minimumText = try scaled(attributed, by: minimum)
        let minimumLayout = try layout(
            minimumText,
            width: width,
            height: height,
            maximumContainerHeight: height,
            adjustsContainerHeight: false,
            verticalAlignment: request.verticalAlignment
        )
        if minimumLayout.fitsBounds {
            var lower = minimum
            var upper: CGFloat = 1
            var best = minimumLayout
            for _ in 0 ..< binarySearchIterations {
                let candidate = (lower + upper) / 2
                let candidateText = try scaled(attributed, by: candidate)
                let candidateLayout = try layout(
                    candidateText,
                    width: width,
                    height: height,
                    maximumContainerHeight: height,
                    adjustsContainerHeight: false,
                    verticalAlignment: request.verticalAlignment
                )
                if candidateLayout.fitsBounds {
                    lower = candidate
                    best = candidateLayout
                } else {
                    upper = candidate
                }
            }
            effectiveScale = Double(lower)
            measured = best
        } else {
            effectiveScale = request.minimumScale
            measured = minimumLayout
        }
    }

    let effectiveText = try scaled(attributed, by: CGFloat(effectiveScale))
    return Evidence(
        fitsBounds: measured.fitsBounds,
        usedRect: UsedRect(
            x: Double(measured.usedRect.minX),
            y: Double(measured.usedRect.minY),
            width: Double(measured.usedRect.width),
            height: Double(measured.usedRect.height)
        ),
        lineCount: measured.lineCount,
        metricStyleRunCount: metricStyleRunCount(in: effectiveText),
        fittedUtf16Range: Utf16Range(
            location: measured.fittedRange.location,
            length: measured.fittedRange.length
        ),
        inputUtf16Length: measured.inputLength,
        effectiveScale: effectiveScale,
        resolvedFonts: try resolvedFonts(in: effectiveText, fontPrograms: fontPrograms),
        nativeLayoutRuntime: try nativeLayoutRuntime()
    )
}

private let decoder: JSONDecoder = {
    let value = JSONDecoder()
    value.keyDecodingStrategy = .convertFromSnakeCase
    return value
}()

private let encoder: JSONEncoder = {
    let value = JSONEncoder()
    value.keyEncodingStrategy = .convertToSnakeCase
    value.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    return value
}()

private func emit(_ response: Response) {
    do {
        let data = try encoder.encode(response)
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([0x0a]))
    } catch {
        FileHandle.standardError.write(Data("failed to encode text-fit response\n".utf8))
        exit(70)
    }
}

private let fontPrograms = FontProgramDigests()
while let line = readLine() {
    let data = Data(line.utf8)
    let requestId = (try? decoder.decode(Request.self, from: data).requestId) ?? 0
    do {
        let request = try decoder.decode(Request.self, from: data)
        emit(
            .success(
                requestId: request.requestId,
                evidence: try measure(request, fontPrograms: fontPrograms)
            )
        )
    } catch let error as OracleError {
        emit(.failure(requestId: requestId, error: error))
    } catch {
        emit(
            .failure(
                requestId: requestId,
                error: OracleError(code: "invalid_request", message: error.localizedDescription)
            )
        )
    }
}
