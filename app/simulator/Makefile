.PHONY: test-yosys
test-yosys:
	docker run --rm -v $(shell pwd):/app -w /app hdlc/yosys yosys -p "read_verilog test.v; synth; opt; fsm; opt; memory; opt; abc -lut 2 -dress; opt; write_json design.json"
