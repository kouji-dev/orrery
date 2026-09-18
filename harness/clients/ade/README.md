# ADE renderer — pointer

There is no crate or package here.

The Angular + kouji-ui renderer is built **inside [`../../../ade/`](../../../ade)**, on top of
[`@orrery/client`](../sdk-ts) (the TypeScript `AguiSession` + `SurfaceStore`) and the
generated types in [`@orrery/protocol`](../../protocol).

It is a client like any other: it attaches to a kernel over the transport and draws what the
AG-UI stream tells it to. It gets no privileged path into the kernel.

This folder exists so the one-renderer-per-folder rule in [`../README.md`](../README.md)
has no hole in it.
