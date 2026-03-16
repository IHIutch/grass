const path = require("path");
const grass = require("./index.js");

const entry = path.resolve(__dirname, "../../prototype/packages/uswds/_index-direct.scss");
const opts = { loadPaths: [path.resolve(__dirname, "../../prototype/packages")] };

const mode = process.argv[2] || "sequential";

// if (mode === "parallel") {
grass.compileParallel(entry, opts);
// } else {
//   grass.compile(entry, opts);
// }
