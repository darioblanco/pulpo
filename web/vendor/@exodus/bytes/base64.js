'use strict';

function toBase64(input) {
  return Buffer.from(input).toString('base64');
}

module.exports = {
  toBase64,
};
