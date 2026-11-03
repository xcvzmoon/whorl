# syntax=docker/dockerfile:1
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS build
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends cmake libleptonica-dev && rm -rf /var/lib/apt/lists/*
COPY spikes/extraction/assets/tesseract.tar.gz /build/tesseract.tar.gz
RUN tar -xzf tesseract.tar.gz && cmake -S tesseract-5.5.4 -B tesseract-build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DBUILD_TRAINING_TOOLS=OFF -DBUILD_TESTS=OFF -DDISABLE_CURL=ON -DDISABLE_ARCHIVE=ON && cmake --build tesseract-build -j2 && cmake --install tesseract-build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches
RUN --mount=type=cache,target=/usr/local/cargo/registry cargo build --release --locked --bin whorl-extract -j2

FROM build AS tests
COPY tests ./tests
COPY spikes/extraction/manifest.json ./spikes/extraction/manifest.json
COPY spikes/extraction/fixtures ./spikes/extraction/fixtures
COPY spikes/extraction/generated ./spikes/extraction/generated
RUN --mount=type=cache,target=/usr/local/cargo/registry cargo test --release --locked --lib --test extraction -j2

FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3 AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends util-linux liblept5 libgomp1 fonts-dejavu-core && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/whorl-extract /usr/local/bin/whorl-extract
COPY --from=build /usr/local/bin/tesseract /usr/local/bin/tesseract
COPY spikes/extraction/assets/pdfium-linux-arm64.tgz /opt/pdfium/archive.tgz
RUN tar -xzf /opt/pdfium/archive.tgz -C /opt/pdfium && rm /opt/pdfium/archive.tgz
COPY spikes/extraction/assets/*.onnx /opt/models/
COPY spikes/extraction/assets/eng.traineddata /opt/tessdata/eng.traineddata
ENV WHORL_PDFIUM_LIBRARY=/opt/pdfium/lib/libpdfium.so WHORL_OCR_DETECTION_MODEL=/opt/models/text-detection.onnx WHORL_OCR_RECOGNITION_MODEL=/opt/models/text-recognition.onnx TESSDATA_PREFIX=/opt/tessdata OMP_THREAD_LIMIT=2 RAYON_NUM_THREADS=2 RTEN_NUM_THREADS=2
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/whorl-extract"]

FROM runtime AS verification
USER root
RUN apt-get update && apt-get install -y --no-install-recommends python3 time && rm -rf /var/lib/apt/lists/*
USER 10001:10001
ENTRYPOINT ["python3", "/verify-extraction.py"]
