'use strict';

const utf8Decoder = new TextDecoder('utf-8', { ignoreBOM: false, fatal: false });
const utf16leDecoder = new TextDecoder('utf-16le', { ignoreBOM: false, fatal: false });
const utf16beDecoder = new TextDecoder('utf-16be', { ignoreBOM: false, fatal: false });

const LABELS = new Map([
  ['csisolatin1', 'windows-1252'],
  ['ibm819', 'windows-1252'],
  ['iso-8859-1', 'windows-1252'],
  ['iso-ir-100', 'windows-1252'],
  ['iso8859-1', 'windows-1252'],
  ['iso88591', 'windows-1252'],
  ['iso_8859-1', 'windows-1252'],
  ['iso_8859-1:1987', 'windows-1252'],
  ['l1', 'windows-1252'],
  ['latin1', 'windows-1252'],
  ['us-ascii', 'windows-1252'],
  ['utf-8', 'UTF-8'],
  ['utf8', 'UTF-8'],
  ['utf-16', 'UTF-16LE'],
  ['utf-16be', 'UTF-16BE'],
  ['utf-16le', 'UTF-16LE'],
  ['windows-1252', 'windows-1252'],
  ['x-user-defined', 'x-user-defined'],
]);

function normalizeLabel(label) {
  if (typeof label !== 'string') {
    return null;
  }

  const normalized = label.trim().toLowerCase();
  return LABELS.get(normalized) || null;
}

function getBOMEncoding(input) {
  if (!input || input.length < 2) {
    return null;
  }

  if (input.length >= 3 && input[0] === 0xef && input[1] === 0xbb && input[2] === 0xbf) {
    return 'UTF-8';
  }

  if (input[0] === 0xfe && input[1] === 0xff) {
    return 'UTF-16BE';
  }

  if (input[0] === 0xff && input[1] === 0xfe) {
    return 'UTF-16LE';
  }

  return null;
}

function decodeWindows1252(input) {
  return Buffer.from(input).toString('latin1');
}

function encodeWindows1252(input) {
  return Uint8Array.from(Buffer.from(input, 'latin1'));
}

function decode(input, label) {
  const canonical = normalizeLabel(label) || 'UTF-8';

  switch (canonical) {
    case 'UTF-8':
      return utf8Decoder.decode(input);
    case 'UTF-16BE':
      return utf16beDecoder.decode(input);
    case 'UTF-16LE':
      return utf16leDecoder.decode(input);
    case 'windows-1252':
    case 'x-user-defined':
      return decodeWindows1252(input);
    default:
      return utf8Decoder.decode(input);
  }
}

function encode(input, label) {
  const canonical = normalizeLabel(label) || 'UTF-8';

  switch (canonical) {
    case 'windows-1252':
    case 'x-user-defined':
      return encodeWindows1252(input);
    case 'UTF-16BE':
      return swapUtf16Endianness(Uint8Array.from(Buffer.from(input, 'utf16le')));
    case 'UTF-16LE':
      return Uint8Array.from(Buffer.from(input, 'utf16le'));
    case 'UTF-8':
    default:
      return Uint8Array.from(Buffer.from(input, 'utf8'));
  }
}

function swapUtf16Endianness(input) {
  const output = new Uint8Array(input);
  for (let i = 0; i + 1 < output.length; i += 2) {
    const first = output[i];
    output[i] = output[i + 1];
    output[i + 1] = first;
  }
  return output;
}

module.exports = {
  decode,
  encode,
  getBOMEncoding,
  normalizeLabel,
};
