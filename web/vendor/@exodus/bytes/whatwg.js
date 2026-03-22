'use strict';

const { encode, normalizeLabel } = require('./shared.js');

function percentEncodeAfterEncoding(label, input, shouldPercentEncode) {
  const canonical = normalizeLabel(label) || 'UTF-8';
  const bytes = encode(input, canonical);
  let output = '';

  for (const byte of bytes) {
    if (!shouldPercentEncode(byte)) {
      output += String.fromCharCode(byte);
      continue;
    }

    output += `%${byte.toString(16).toUpperCase().padStart(2, '0')}`;
  }

  return output;
}

module.exports = {
  percentEncodeAfterEncoding,
};
