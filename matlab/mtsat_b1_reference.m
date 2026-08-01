% mtsat_b1_reference.m
%
% Reference-generation harness for validating qmrust's MTsat B1+ FLASH Bloch
% engine (crates/qmrust-core/src/mtsat_b1/) against TardifLab's
% BlochSimFlashSequence_v2, the MATLAB implementation qmrust ports.
%
% This script simulates, at every point of a fixed (M0b, b1, R1obs) grid, the
% single-echo FLASH signal for three acquisitions of a single-point MTsat
% protocol (MT-weighted, and the two VFA points used for the Helms apparent
% R1/A0), and writes them to matlab/mtsat_b1_reference.csv. A separate Rust
% test loads that CSV and compares it voxel-by-voxel against qmrust's own
% simulated signals; any numerical mismatch is a porting regression.
%
% Required MATLAB toolboxes (add to path before running):
%   - OptimizeIHMTimaging  (BlochSimFlashSequence_v2 and its Bloch/pulse helpers)
%   - qMRLab                (GetPulse, pulse-shape functions, computeG)
%   - NeuroImagingMatlab    (transitively required by OptimizeIHMTimaging)
%
% Run with:
%   >> setupIHMTsimPaths   % or otherwise add the three toolboxes to path
%   >> mtsat_b1_reference
%
% WARNING: every value in the "SHARED FIXED-PARAMS TABLE" block below MUST
% stay bit-for-bit identical to the Rust reference test's SeqParams/VfaParams
% (crates/qmrust-core/src/mtsat_b1/sim.rs, tests_sample_params()). If either
% side changes, update both and regenerate the CSV.

clear; clc;
scriptTimer = tic;

%% ------------------------------------------------------------------
%  SHARED FIXED-PARAMS TABLE -- MUST stay identical to the Rust test config
%  ------------------------------------------------------------------
T2a         = 0.070;    % Params.T2a (s), free-water T2
T2b         = 12e-6;    % Params.T2b (s), bound-pool T2
T1D         = 6e-3;     % Params.T1D (s), dipolar-order relaxation time
R           = 26;       % Params.R (1/s), free<->bound exchange rate constant
R1b         = 1;        % Params.R1b (1/s), bound-pool longitudinal relaxation rate
M0a         = 1;        % Params.M0a, free-water equilibrium magnetization
flipAngle   = 6;         % Params.flipAngle (deg), MTw excitation flip angle
TR          = 0.028;    % Params.TR (s), MTw repetition time
pulseDur    = 0.768e-3; % Params.pulseDur (s), saturation-pulse duration
pulseGapDur = 0.6e-3;   % Params.pulseGapDur (s), gap between saturation pulses
delta       = 7000;     % Params.delta (Hz), saturation-pulse offset frequency
numSatPulse = 2;        % Params.numSatPulse, saturation pulses per TR
freqPattern = 'dualAlternate'; % Params.freqPattern; also supports 'single'
WExcDur     = 3e-3;     % Params.WExcDur (s), water excitation-pulse duration
bw          = 0.3 / pulseDur; % Params.PulseOpt.bw (Hz), gausshann pulse bandwidth
mt_grad_time = 0;       % Params.G_time_elapse_MT (s), post-train MT-spoil gradient
                         % interval; 0 => pure (instantaneous) spoil, no added relaxation
Navg        = 20;       % matches BlochSimFlashSequence_v2's hard-coded
                         % num2avgOver (not a Params field); documented here
                         % only so the two implementations' averaging windows
                         % are known to agree.

% VFA acquisitions (unsaturated FLASH pair for the Helms apparent R1/A0)
fa1 = 5;      % Params.flipAngle (deg) for the PDw (low-flip) VFA point
fa2 = 20;     % Params.flipAngle (deg) for the T1w (high-flip) VFA point
tr1 = 0.030;  % Params.TR (s) for the PDw VFA point
tr2 = 0.030;  % Params.TR (s) for the T1w VFA point

%% ------------------------------------------------------------------
%  Fixed validation grid
%  ------------------------------------------------------------------
M0b_grid = [0.02, 0.06, 0.10, 0.14, 0.18];
b1_grid  = [2, 4, 6.8, 9];          % microTesla rms
R1_grid  = [1/2.0, 1/1.2, 1/0.8, 1/0.6]; % 1/s (Raobs)

%% ------------------------------------------------------------------
%  Base Params: single-echo FLASH configuration
%  ------------------------------------------------------------------
% This engine restricts to single-echo FLASH via numExcitation=1, DummyEcho=0.
% For MTC=0 (VFA) with numExcitation==1, BlochSimFlashSequence_v2 itself
% forces echoSpacing=TR and DummyEcho=0 (its single-echo FLASH reduction);
% leaving echoSpacing at 0 here lets that reduction (and the engine's
% fallback echoSpacing=5e-3 default for the MTC=1 path) take effect rather
% than guessing a value the engine would silently override.
%
% For MTC=1 the choice of echoSpacing is provably inconsequential: with
% numExcitation=1 the engine's own TD formula gives
%   echoSpacing + TD = TR - numSatPulse*(pulseDur+pulseGapDur) + pulseGapDur - G_time_elapse_MT
% independent of echoSpacing (it cancels), and both segments are free
% relaxation under the same generator matrix, so splitting that fixed total
% duration between echoSpacing and TD at any point (as long as TD >= 0)
% leaves the steady-state signal unchanged.
%
% TODO(collaborator): two structural timing points to confirm before trusting
% a tight numeric tolerance in the Rust-vs-MATLAB comparison:
%  1. BlochSimFlashSequence_v2 treats excitation as instantaneous and never
%     reserves WExcDur of real relaxation time anywhere in the surrounding
%     echoSpacing/TD/tr-fill blocks. The Rust port's tr_fill instead
%     subtracts w_exc_dur from the modeled relaxation budget (i.e. it
%     reserves WExcDur as unmodeled dead time). With WExcDur=3ms and
%     TR=28ms this is a ~10% difference in modeled free-relaxation time per
%     TR and is NOT obviously a rounding issue -- confirm which convention
%     is intended, or budget for a corresponding tolerance.
%  2. For the MTC=1, numExcitation=1 reduction, the upstream TD formula's
%     "+ pulseGapDur" term makes the true real elapsed time per TR equal to
%     TR + pulseGapDur (0.6ms here), not TR, regardless of echoSpacing (see
%     derivation above). This is an upstream quirk in
%     BlochSimFlashSequence_v2 itself (likely tuned for its turbo-factor
%     multi-echo case), not something introduced by this harness -- confirm
%     whether the Rust port intentionally reproduces this ~2% overrun for
%     the MT-weighted acquisition.
base = struct();
base.M0a          = M0a;
base.R            = R;
base.R1b          = R1b;
base.T2a          = T2a;
base.T2b          = T2b;
base.T1D          = T1D;
base.lineshape     = 'SuperLorentzian'; % matches the Rust engine's rate
                                          % matrix, which hard-codes the
                                          % super-Lorentzian wloc = sqrt(1/(15*T2b^2))
base.IncludeDipolar = 1;
base.PerfectSpoiling = 1;
% RF-spoiling phase is not modeled by the Rust engine. Under PerfectSpoiling
% with a single isochromat, the excitation-phase increment cancels out of the
% transverse-magnitude signal anyway (it rotates a purely longitudinal
% magnetization, and |M_xy| = |sin(flipAngle)*Mz| is phase-independent), so
% disabling it here matches the Rust engine exactly rather than by accident.
base.RFspoiling    = false;
base.CalcVector    = 0;
base.WExcDur       = WExcDur;
base.echoSpacing   = 0;
base.numExcitation = 1;
base.DummyEcho     = 0;
base.boosted       = 0;
base.delta         = delta;
base.freqPattern   = freqPattern;
base.numSatPulse   = numSatPulse;
base.pulseDur      = pulseDur;
base.pulseGapDur   = pulseGapDur;
base.SatPulseShape = 'gausshann';
base.PulseOpt.bw   = bw;
base.G_time_elapse_MT = mt_grad_time;

%% ------------------------------------------------------------------
%  Grid loop
%  ------------------------------------------------------------------
nRows = numel(M0b_grid) * numel(b1_grid) * numel(R1_grid);
results = zeros(nRows, 7); % M0b, b1, R1, satFlipAngle, sig_MTw, sig_PDw, sig_T1w
row = 0;

for iM = 1:numel(M0b_grid)
    M0b = M0b_grid(iM);

    for iR = 1:numel(R1_grid)
        R1obs = R1_grid(iR);

        for iB = 1:numel(b1_grid)
            b1 = b1_grid(iB);
            row = row + 1;

            % Solve satFlipAngle so the gausshann pulse's RMS B1 equals the
            % target b1 (microTesla). getPulseB1rms's exported B1rms is
            % linear in satFlipAngle for a fixed pulse shape/duration (the
            % pulse amplitude it computes via GetPulse is itself linear in
            % the requested flip angle), so a single-point linear estimate
            % is used to seed fzero, then fzero polishes the root.
            %
            % TODO(collaborator): getPulseB1rms.m hard-codes its own
            % gausshann bandwidth (PulseOpt.bw = 0.0002/pulseDur) internally
            % and does not accept the PulseOpt used by the actual saturation
            % train (bw = 0.3/pulseDur, set above). This means satFlipAngle
            % is solved against a narrower reference pulse than the one
            % actually played in BlochSimFlashSequence_v2. This mirrors an
            % existing inconsistency in the upstream OptimizeIHMTimaging
            % toolbox (getPulseB1rms.m does not expose a PulseOpt argument);
            % confirm whether this reference should instead compute B1rms
            % with bw=0.3/pulseDur to match the sim pulse exactly.
            b1rms_per_deg = getPulseB1rms(1, pulseDur, base.SatPulseShape);
            guess = b1 / b1rms_per_deg;
            satFlipAngle = fzero(@(fa) getPulseB1rms(fa, pulseDur, base.SatPulseShape) - b1, guess);

            Params = base;
            Params.Raobs = R1obs;
            Params.M0b   = M0b;

            % MTw: saturated FLASH at the nominal excitation flip angle.
            sig_MTw = BlochSimFlashSequence_v2(Params, ...
                'MTC', 1, 'satFlipAngle', satFlipAngle, ...
                'flipAngle', flipAngle, 'TR', TR);

            % PDw (VFA low flip): no saturation pulses.
            sig_PDw = BlochSimFlashSequence_v2(Params, ...
                'MTC', 0, 'flipAngle', fa1, 'TR', tr1);

            % T1w (VFA high flip): no saturation pulses.
            sig_T1w = BlochSimFlashSequence_v2(Params, ...
                'MTC', 0, 'flipAngle', fa2, 'TR', tr2);

            results(row, :) = [M0b, b1, R1obs, satFlipAngle, sig_MTw, sig_PDw, sig_T1w];

            fprintf('[%3d/%3d] M0b=%.2f b1=%.1f R1=%.3f -> satFlipAngle=%.3f (%.1f s elapsed)\n', ...
                row, nRows, M0b, b1, R1obs, satFlipAngle, toc(scriptTimer));
        end
    end
end

%% ------------------------------------------------------------------
%  Write CSV
%  ------------------------------------------------------------------
outPath = fullfile(fileparts(mfilename('fullpath')), 'mtsat_b1_reference.csv');
fid = fopen(outPath, 'w');
fprintf(fid, 'M0b,b1,R1,satFlipAngle,sig_MTw,sig_PDw,sig_T1w\n');
fclose(fid);
writematrix(results, outPath, 'WriteMode', 'append');

elapsed = toc(scriptTimer);
fprintf('Wrote %d rows to %s\n', nRows, outPath);
fprintf('Total run time: %.1f s (%.2f min)\n', elapsed, elapsed / 60);
