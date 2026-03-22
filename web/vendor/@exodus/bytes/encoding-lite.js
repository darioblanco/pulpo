'use strict';

const { decode, getBOMEncoding, normalizeLabel } = require('./shared.js');

function labelToName(label) {
  return normalizeLabel(label);
}

function legacyHookDecode(input, label) {
  return decode(input, label);
}

module.exports = {
  TextDecoder,
  TextEncoder,
  getBOMEncoding,
  labelToName,
  legacyHookDecode,
};
