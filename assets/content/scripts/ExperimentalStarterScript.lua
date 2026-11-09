local CoreGui = game:GetService("CoreGui")
local Players = game:GetService("Players")
local UserInputService = game:GetService("UserInputService")
local ContextActionService = game:GetService("ContextActionService")
local GuiService = game:GetService("GuiService")

local INK = Color3.new(20/255, 26/255, 51/255)
local INK_RAISED = Color3.new(34/255, 42/255, 78/255)
local AMBER = Color3.new(255/255, 177/255, 59/255)
local PAPER = Color3.new(242/255, 244/255, 250/255)
local MUTED = Color3.new(160/255, 168/255, 198/255)
local DANGER = Color3.new(255/255, 99/255, 92/255)
local GLASS = 0.15

local player = Players.LocalPlayer
while not player do
	wait()
	player = Players.LocalPlayer
end

local gui = Instance.new("ScreenGui")
gui.Name = "ExperimentalGui"
gui.Parent = CoreGui

local function panel(name, size, position, parent)
	local f = Instance.new("Frame")
	f.Name = name
	f.Size = size
	f.Position = position
	f.BackgroundColor3 = INK
	f.BackgroundTransparency = GLASS
	f.BorderSizePixel = 0
	f.Parent = parent
	return f
end

local function label(name, text, size, position, font, textSize, color, parent)
	local t = Instance.new("TextLabel")
	t.Name = name
	t.Text = text
	t.Size = size
	t.Position = position
	t.BackgroundTransparency = 1
	t.Font = font
	t.FontSize = textSize
	t.TextColor3 = color
	t.TextXAlignment = Enum.TextXAlignment.Left
	t.Parent = parent
	return t
end

local function button(name, text, size, position, parent)
	local b = Instance.new("TextButton")
	b.Name = name
	b.Text = text
	b.Size = size
	b.Position = position
	b.BackgroundColor3 = INK_RAISED
	b.BackgroundTransparency = 0
	b.BorderSizePixel = 0
	b.AutoButtonColor = false
	b.Font = Enum.Font.SourceSansBold
	b.FontSize = Enum.FontSize.Size18
	b.TextColor3 = PAPER
	b.Parent = parent
	b.MouseEnter:connect(function()
		b.BackgroundColor3 = AMBER
		b.TextColor3 = INK
	end)
	b.MouseLeave:connect(function()
		b.BackgroundColor3 = INK_RAISED
		b.TextColor3 = PAPER
	end)
	return b
end

-- Topbar
local pill = panel("Topbar", UDim2.new(0, 104, 0, 40), UDim2.new(0, 12, 0, 10), gui)

local menuButton = Instance.new("TextButton")
menuButton.Name = "MenuButton"
menuButton.Text = ""
menuButton.Size = UDim2.new(0, 40, 0, 40)
menuButton.BackgroundTransparency = 1
menuButton.Parent = pill
for i = 0, 2 do
	local bar = Instance.new("Frame")
	bar.Size = UDim2.new(0, 18, 0, 2)
	bar.Position = UDim2.new(0, 11, 0, 13 + i * 6)
	bar.BackgroundColor3 = PAPER
	bar.BorderSizePixel = 0
	bar.Parent = menuButton
end

local divider = Instance.new("Frame")
divider.Size = UDim2.new(0, 1, 0, 22)
divider.Position = UDim2.new(0, 46, 0, 9)
divider.BackgroundColor3 = MUTED
divider.BackgroundTransparency = 0.6
divider.BorderSizePixel = 0
divider.Parent = pill

local chatButton = Instance.new("TextButton")
chatButton.Name = "ChatButton"
chatButton.Text = ""
chatButton.Size = UDim2.new(0, 52, 0, 40)
chatButton.Position = UDim2.new(0, 52, 0, 0)
chatButton.BackgroundTransparency = 1
chatButton.Parent = pill
local bubble = Instance.new("Frame")
bubble.Size = UDim2.new(0, 22, 0, 15)
bubble.Position = UDim2.new(0, 12, 0, 11)
bubble.BackgroundColor3 = AMBER
bubble.BorderSizePixel = 0
bubble.Parent = chatButton
local tail = Instance.new("Frame")
tail.Size = UDim2.new(0, 6, 0, 6)
tail.Position = UDim2.new(0, 14, 0, 24)
tail.Rotation = 45
tail.BackgroundColor3 = AMBER
tail.BorderSizePixel = 0
tail.Parent = chatButton

local status = panel("Status", UDim2.new(0, 200, 0, 40), UDim2.new(1, -212, 0, 10), gui)
label("PlayerName", player.Name, UDim2.new(1, -24, 0, 18), UDim2.new(0, 12, 0, 4),
	Enum.Font.SourceSansBold, Enum.FontSize.Size18, PAPER, status)
local healthTrack = Instance.new("Frame")
healthTrack.Name = "HealthTrack"
healthTrack.Size = UDim2.new(1, -24, 0, 5)
healthTrack.Position = UDim2.new(0, 12, 0, 27)
healthTrack.BackgroundColor3 = INK_RAISED
healthTrack.BorderSizePixel = 0
healthTrack.Parent = status
local healthFill = Instance.new("Frame")
healthFill.Name = "HealthFill"
healthFill.Size = UDim2.new(1, 0, 1, 0)
healthFill.BackgroundColor3 = AMBER
healthFill.BorderSizePixel = 0
healthFill.Parent = healthTrack

local function trackHumanoid(character)
	local humanoid = character:WaitForChild("Humanoid")
	local function update()
		local ratio = humanoid.MaxHealth > 0 and humanoid.Health / humanoid.MaxHealth or 0
		ratio = math.max(0, math.min(1, ratio))
		healthFill:TweenSize(UDim2.new(ratio, 0, 1, 0), Enum.EasingDirection.Out, Enum.EasingStyle.Quad, 0.2, true)
		healthFill.BackgroundColor3 = ratio < 0.3 and DANGER or AMBER
	end
	humanoid.HealthChanged:connect(update)
	update()
end
if player.Character then
	spawn(function() trackHumanoid(player.Character) end)
end
player.CharacterAdded:connect(function(c) trackHumanoid(c) end)

-- Chat
local MAX_LINES = 8
local LINE_HEIGHT = 22
local chatLog = Instance.new("Frame")
chatLog.Name = "ChatLog"
chatLog.Size = UDim2.new(0, 420, 0, MAX_LINES * LINE_HEIGHT)
chatLog.Position = UDim2.new(0, 12, 1, -(MAX_LINES * LINE_HEIGHT) - 72)
chatLog.BackgroundTransparency = 1
chatLog.Parent = gui

local lines = {}
local function relayout()
	for i, line in ipairs(lines) do
		local y = (MAX_LINES - (#lines - i + 1)) * LINE_HEIGHT
		line:TweenPosition(UDim2.new(0, 0, 0, y), Enum.EasingDirection.Out, Enum.EasingStyle.Quad, 0.15, true)
	end
end

local function addLine(speaker, message)
	local row = panel("Line", UDim2.new(1, 0, 0, LINE_HEIGHT - 2), UDim2.new(0, 0, 0, MAX_LINES * LINE_HEIGHT), chatLog)
	row.BackgroundTransparency = 0.35
	local name = label("Speaker", speaker, UDim2.new(0, 100, 1, 0), UDim2.new(0, 8, 0, 0),
		Enum.Font.SourceSansBold, Enum.FontSize.Size18, AMBER, row)
	name.TextXAlignment = Enum.TextXAlignment.Left
	local nameWidth = math.min(140, name.TextBounds.X + 8)
	name.Size = UDim2.new(0, nameWidth, 1, 0)
	label("Message", message, UDim2.new(1, -nameWidth - 20, 1, 0), UDim2.new(0, nameWidth + 12, 0, 0),
		Enum.Font.SourceSans, Enum.FontSize.Size18, PAPER, row)
	table.insert(lines, row)
	if #lines > MAX_LINES then
		table.remove(lines, 1):Destroy()
	end
	relayout()
end

local function hookPlayer(p)
	p.Chatted:connect(function(message)
		addLine(p.Name, message)
	end)
end
for _, p in ipairs(Players:GetPlayers()) do
	hookPlayer(p)
end
Players.PlayerAdded:connect(function(p)
	hookPlayer(p)
	addLine("Server", p.Name .. " joined the game")
end)
Players.PlayerRemoving:connect(function(p)
	addLine("Server", p.Name .. " left the game")
end)

local inputBar = panel("ChatBar", UDim2.new(0, 420, 0, 36), UDim2.new(0, 12, 1, -60), gui)
inputBar.Visible = false
local inputBox = Instance.new("TextBox")
inputBox.Name = "ChatInput"
inputBox.Text = ""
inputBox.Size = UDim2.new(1, -24, 1, 0)
inputBox.Position = UDim2.new(0, 12, 0, 0)
inputBox.BackgroundTransparency = 1
inputBox.Font = Enum.Font.SourceSans
inputBox.FontSize = Enum.FontSize.Size18
inputBox.TextColor3 = PAPER
inputBox.TextXAlignment = Enum.TextXAlignment.Left
inputBox.ClearTextOnFocus = true
inputBox.Parent = inputBar
local accentRule = Instance.new("Frame")
accentRule.Size = UDim2.new(1, 0, 0, 2)
accentRule.Position = UDim2.new(0, 0, 1, -2)
accentRule.BackgroundColor3 = AMBER
accentRule.BorderSizePixel = 0
accentRule.Parent = inputBar

local function openChat()
	inputBar.Visible = true
	inputBox.Text = ""
	inputBox:CaptureFocus()
end

inputBox.FocusLost:connect(function(enterPressed)
	local text = inputBox.Text
	inputBar.Visible = false
	if enterPressed and text ~= "" then
		pcall(function() Players:Chat(text) end)
	end
end)

chatButton.MouseButton1Click:connect(openChat)
UserInputService.InputBegan:connect(function(input, processed)
	if processed then return end
	if input.KeyCode == Enum.KeyCode.Slash then
		openChat()
	end
end)

addLine("Server", "Press / to chat")

-- Escape menu
local shade = Instance.new("Frame")
shade.Name = "MenuShade"
shade.Size = UDim2.new(1, 0, 1, 0)
shade.BackgroundColor3 = Color3.new(0, 0, 0)
shade.BackgroundTransparency = 0.45
shade.BorderSizePixel = 0
shade.Visible = false
shade.Active = true
shade.Parent = gui

local card = panel("MenuCard", UDim2.new(0, 320, 0, 252), UDim2.new(0.5, -160, 0.5, -126), shade)
card.BackgroundTransparency = 0
label("Title", "Paused", UDim2.new(1, -40, 0, 36), UDim2.new(0, 20, 0, 16),
	Enum.Font.SourceSansBold, Enum.FontSize.Size28, PAPER, card)
label("Place", "You can pick up where you left off.", UDim2.new(1, -40, 0, 20), UDim2.new(0, 20, 0, 50),
	Enum.Font.SourceSans, Enum.FontSize.Size18, MUTED, card)

local resume = button("Resume", "Resume", UDim2.new(1, -40, 0, 40), UDim2.new(0, 20, 0, 88), card)
local reset = button("Reset", "Reset character", UDim2.new(1, -40, 0, 40), UDim2.new(0, 20, 0, 138), card)
local leave = button("Leave", "Leave game", UDim2.new(1, -40, 0, 40), UDim2.new(0, 20, 0, 188), card)
leave.TextColor3 = DANGER
leave.MouseLeave:connect(function() leave.TextColor3 = DANGER end)

local menuOpen = false
local function setMenu(open)
	menuOpen = open
	pcall(function() GuiService:SetMenuIsOpen(open) end)
	if open then
		shade.Visible = true
		card.Position = UDim2.new(0.5, -160, 0.5, -96)
		card:TweenPosition(UDim2.new(0.5, -160, 0.5, -126), Enum.EasingDirection.Out, Enum.EasingStyle.Quad, 0.18, true)
	else
		shade.Visible = false
	end
end

resume.MouseButton1Click:connect(function() setMenu(false) end)
reset.MouseButton1Click:connect(function()
	setMenu(false)
	local character = player.Character
	local humanoid = character and character:FindFirstChild("Humanoid")
	if humanoid then humanoid.Health = 0 end
end)
leave.MouseButton1Click:connect(function()
	pcall(function() game:Shutdown() end)
end)
menuButton.MouseButton1Click:connect(function() setMenu(not menuOpen) end)

local bound = pcall(function()
	ContextActionService:BindCoreAction("ExperimentalMenu", function(_, state)
		if state == Enum.UserInputState.Begin then
			setMenu(not menuOpen)
		end
	end, false, Enum.KeyCode.Escape)
end)
if not bound then
	UserInputService.InputBegan:connect(function(input)
		if input.KeyCode == Enum.KeyCode.Escape then
			setMenu(not menuOpen)
		end
	end)
end
