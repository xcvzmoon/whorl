# syntax=docker/dockerfile:1
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS build
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends cmake libleptonica-dev && rm -rf /var/lib/apt/lists/*
COPY assets/tesseract.tar.gz /build/tesseract.tar.gz
RUN tar -xzf tesseract.tar.gz && cmake -S tesseract-5.5.4 -B tesseract-build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DBUILD_TRAINING_TOOLS=OFF -DBUILD_TESTS=OFF -DDISABLE_CURL=ON -DDISABLE_ARCHIVE=ON && cmake --build tesseract-build -j2 && cmake --install tesseract-build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry cargo build --release --locked -j2

FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3 AS runtime
WORKDIR /corpus
RUN apt-get update && apt-get install -y --no-install-recommends python3 time liblept5 libgomp1 fonts-dejavu-core && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/extraction-spike /usr/local/bin/extraction-spike
COPY --from=build /usr/local/bin/tesseract /usr/local/bin/tesseract
COPY assets/pdfium-linux-arm64.tgz /opt/pdfium/archive.tgz
RUN tar -xzf /opt/pdfium/archive.tgz -C /opt/pdfium && rm /opt/pdfium/archive.tgz
COPY assets/*.onnx /opt/models/
COPY assets/eng.traineddata /opt/tessdata/eng.traineddata
ENV PDFIUM_LIBRARY=/opt/pdfium/lib/libpdfium.so OCR_MODELS=/opt/models TESSDATA_PREFIX=/opt/tessdata OMP_THREAD_LIMIT=2 RAYON_NUM_THREADS=2 RTEN_NUM_THREADS=2
USER 10001:10001
ENTRYPOINT ["python3", "/corpus/benchmark.py"]
