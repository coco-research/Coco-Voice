if (import.meta.env.VITE_MEETINGS === "1") {
  void import("./OfferWindowApp").then((mod) => mod.mountOfferWindow());
}
