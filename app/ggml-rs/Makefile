build-ggml-rs:
	GGML_PATH=./ggml/ cargo build

test-ggml-rs:
	./ggml/build/bin/test-backend-ops

build-ggml:
	rm -rf ggml/build ggml/install
	mkdir ggml/build ggml/install
	cd ggml/build && cmake -G Ninja -DBUILD_SHARED_LIBS=OFF -DGGML_TTX=ON ..
	cd ggml/build && cmake --build .
