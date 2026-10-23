FROM rust:1.94-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY core ./core
COPY app ./app
RUN cargo build --release --locked -p pdf-editor-core --bin corpus-runner
FROM debian:bookworm-slim AS download
ARG PDFIUM_SHA256=bdc60126399f39fe768566a18d4be055a7b3681bc9e248e8a6656b4f6cae0aa2
RUN apt-get update && apt-get install -y --no-install-recommends python3 ca-certificates curl binutils && rm -rf /var/lib/apt/lists/*
RUN test -n "$PDFIUM_SHA256" && curl -fL --proto '=https' https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8111/pdfium-linux-x64.tgz -o /tmp/pdfium.tgz && echo "$PDFIUM_SHA256  /tmp/pdfium.tgz" | sha256sum -c - && mkdir /opt/pdfium && tar -xzf /tmp/pdfium.tgz -C /opt/pdfium lib/libpdfium.so LICENSE licenses && rm /tmp/pdfium.tgz
# Upstream 05-configure.sh ties both pdf_enable_v8 and pdf_enable_xfa to ENABLE_V8=false for standard archives. FPDF_LoadXFA remains an exported stub.
RUN ! nm -D /opt/pdfium/lib/libpdfium.so | grep -E 'FPDF_GetRecommendedV8Flags|FPDF_GetArrayBufferAllocatorSharedInstance'
COPY scripts/corpus/*.py /opt/pdf-editor/
ENV PDF_EDITOR_CONTAINER=1
USER 65532:65532
CMD ["python3", "/opt/pdf-editor/download.py"]
FROM download AS runner
COPY --from=build /build/target/release/corpus-runner /opt/pdf-editor/corpus-runner
CMD ["python3", "/opt/pdf-editor/run.py"]
