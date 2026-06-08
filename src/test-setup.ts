// Register Angular's JIT compiler for vitest. Specs that import router/common
// (e.g. loading.component.spec) pull in @Injectable declarations that need the
// compiler facade; importing this once globally satisfies them. The AOT build
// (@angular/build) does not use this path.
import '@angular/compiler';
