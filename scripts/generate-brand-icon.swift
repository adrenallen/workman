import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

guard CommandLine.arguments.count == 3 else {
    FileHandle.standardError.write(
        Data("usage: swift scripts/generate-brand-icon.swift <source.png> <output.png>\n".utf8)
    )
    exit(2)
}

let sourceURL = URL(fileURLWithPath: CommandLine.arguments[1])
let outputURL = URL(fileURLWithPath: CommandLine.arguments[2])
let iconSize = 1024

guard
    let source = CGImageSourceCreateWithURL(sourceURL as CFURL, nil),
    let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
    let context = CGContext(
        data: nil,
        width: iconSize,
        height: iconSize,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )
else {
    FileHandle.standardError.write(Data("could not read the source icon\n".utf8))
    exit(1)
}

let bounds = CGRect(x: 0, y: 0, width: iconSize, height: iconSize)
let cornerRadius = CGFloat(iconSize) * 0.2237
context.clear(bounds)
context.interpolationQuality = .high
context.addPath(
    CGPath(
        roundedRect: bounds,
        cornerWidth: cornerRadius,
        cornerHeight: cornerRadius,
        transform: nil
    )
)
context.clip()
context.draw(image, in: bounds)

guard
    let icon = context.makeImage(),
    let destination = CGImageDestinationCreateWithURL(
        outputURL as CFURL,
        UTType.png.identifier as CFString,
        1,
        nil
    )
else {
    FileHandle.standardError.write(Data("could not create the output icon\n".utf8))
    exit(1)
}

CGImageDestinationAddImage(destination, icon, nil)
guard CGImageDestinationFinalize(destination) else {
    FileHandle.standardError.write(Data("could not write the output icon\n".utf8))
    exit(1)
}
