const path = require("path");
const grass = require("./index.js");

const entry = path.resolve(__dirname, "../../prototype/packages/uswds/_index-direct.scss");
const opts = { loadPaths: [path.resolve(__dirname, "../../prototype/packages")] };

grass.compile(entry, opts);
