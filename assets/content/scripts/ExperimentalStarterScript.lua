local CoreGui = game:GetService("CoreGui")
local Players = game:GetService("Players")
local UserInputService = game:GetService("UserInputService")
local ContextActionService = game:GetService("ContextActionService")
local GuiService = game:GetService("GuiService")

local SNOW = Color3.new(1, 1, 1)
local MIST = Color3.new(238/255, 241/255, 247/255)
local RULE = Color3.new(216/255, 221/255, 232/255)
local INK = Color3.new(30/255, 36/255, 51/255)
local SLATE = Color3.new(91/255, 100/255, 120/255)
local BLUE = Color3.new(0, 162/255, 255/255)
local CORAL = Color3.new(255/255, 90/255, 78/255)

local NAME_COLORS = {
	Color3.new(253/255, 41/255, 67/255),
	Color3.new(1/255, 162/255, 255/255),
	Color3.new(2/255, 184/255, 87/255),
	Color3.new(107/255, 50/255, 124/255),
	Color3.new(218/255, 133/255, 65/255),
	Color3.new(193/255, 160/255, 0),
	Color3.new(200/255, 90/255, 160/255),
	Color3.new(160/255, 132/255, 82/255),
}

local ICON_MENU = "rbxasset://textures/ui/Menu/Hamburger@2x.png"
local ICON_CHAT = "rbxasset://textures/ui/Chat/ToggleChat@2x.png"
local ICON_RESET = "rbxasset://textures/ui/Settings/Help/ResetIcon.png"
local ICON_PLAYERS = "rbxasset://textures/ui/Settings/MenuBarIcons/PlayersTabIcon@2x.png"
local ICON_LEAVE = "rbxasset://textures/ui/Settings/MenuBarIcons/HomeTab@2x.png"

local player = Players.LocalPlayer
while not player do
	wait()
	player = Players.LocalPlayer
end

local gui = Instance.new("ScreenGui")
gui.Name = "ExperimentalGui"
gui.Parent = CoreGui

local function frame(name, size, position, color, parent)
	local f = Instance.new("Frame")
	f.Name = name
	f.Size = size
	f.Position = position
	f.BackgroundColor3 = color
	f.BorderSizePixel = 0
	f.Parent = parent
	return f
end

local function card(name, size, position, parent)
	local shadow = frame(name .. "Shadow", size, position + UDim2.new(0, 0, 0, 2), INK, parent)
	shadow.BackgroundTransparency = 0.88
	local f = frame(name, UDim2.new(1, 0, 1, 0), UDim2.new(0, 0, 0, -2), SNOW, shadow)
	return f, shadow
end

local function text(name, value, size, position, font, fontSize, color, parent)
	local t = Instance.new("TextLabel")
	t.Name = name
	t.Text = value
	t.Size = size
	t.Position = position
	t.BackgroundTransparency = 1
	t.Font = font
	t.FontSize = fontSize
	t.TextColor3 = color
	t.TextXAlignment = Enum.TextXAlignment.Left
	t.Parent = parent
	return t
end

local function icon(name, image, size, position, color, parent)
	local i = Instance.new("ImageLabel")
	i.Name = name
	i.Image = image
	i.Size = size
	i.Position = position
	i.BackgroundTransparency = 1
	i.ImageColor3 = color
	i.Parent = parent
	return i
end

local function nameColor(name)
	local value = 0
	for i = 1, #name do
		local byte = string.byte(name, i)
		local reverse = #name - i + 1
		if #name % 2 == 1 then
			reverse = reverse - 1
		end
		if reverse % 4 >= 2 then
			byte = -byte
		end
		value = value + byte
	end
	return NAME_COLORS[(value % #NAME_COLORS) + 1]
end

local function tileButton(name, image, position)
	local tile, shadow = card(name, UDim2.new(0, 40, 0, 40), position, gui)
	local hit = Instance.new("TextButton")
	hit.Name = "Hit"
	hit.Text = ""
	hit.Size = UDim2.new(1, 0, 1, 0)
	hit.BackgroundTransparency = 1
	hit.Parent = tile
	local glyph = icon("Glyph", image, UDim2.new(0, 24, 0, 20), UDim2.new(0.5, -12, 0.5, -10), INK, tile)
	hit.MouseEnter:connect(function()
		tile.BackgroundColor3 = MIST
		glyph.ImageColor3 = BLUE
	end)
	hit.MouseLeave:connect(function()
		tile.BackgroundColor3 = SNOW
		glyph.ImageColor3 = INK
	end)
	return hit, tile, shadow
end

-- Topbar
local menuButton = tileButton("MenuTile", ICON_MENU, UDim2.new(0, 12, 0, 12))
local chatButton, chatTile = tileButton("ChatTile", ICON_CHAT, UDim2.new(0, 60, 0, 12))

local unread = frame("Unread", UDim2.new(0, 18, 0, 18), UDim2.new(1, -11, 0, -7), BLUE, chatTile)
unread.Visible = false
local unreadText = text("Count", "", UDim2.new(1, 0, 1, 0), UDim2.new(0, 0, 0, 0),
	Enum.Font.SourceSansBold, Enum.FontSize.Size14, SNOW, unread)
unreadText.TextXAlignment = Enum.TextXAlignment.Center

local status = card("Status", UDim2.new(0, 220, 0, 40), UDim2.new(1, -232, 0, 12), gui)
text("PlayerName", player.Name, UDim2.new(1, -70, 0, 24), UDim2.new(0, 12, 0, 4),
	Enum.Font.SourceSansBold, Enum.FontSize.Size18, INK, status)
local healthValue = text("HealthValue", "100", UDim2.new(0, 48, 0, 24), UDim2.new(1, -60, 0, 4),
	Enum.Font.SourceSansBold, Enum.FontSize.Size18, SLATE, status)
healthValue.TextXAlignment = Enum.TextXAlignment.Right
local track = frame("HealthTrack", UDim2.new(1, -24, 0, 3), UDim2.new(0, 12, 1, -9), RULE, status)
local fill = frame("HealthFill", UDim2.new(1, 0, 1, 0), UDim2.new(0, 0, 0, 0), BLUE, track)

local function watchHumanoid(character)
	local humanoid = character:WaitForChild("Humanoid")
	local function update()
		local ratio = humanoid.MaxHealth > 0 and humanoid.Health / humanoid.MaxHealth or 0
		ratio = math.max(0, math.min(1, ratio))
		fill:TweenSize(UDim2.new(ratio, 0, 1, 0), Enum.EasingDirection.Out, Enum.EasingStyle.Quart, 0.35, true)
		local low = ratio <= 0.3
		fill.BackgroundColor3 = low and CORAL or BLUE
		healthValue.TextColor3 = low and CORAL or SLATE
		healthValue.Text = tostring(math.floor(humanoid.Health + 0.5))
	end
	humanoid.HealthChanged:connect(update)
	update()
end
if player.Character then
	spawn(function() watchHumanoid(player.Character) end)
end
player.CharacterAdded:connect(watchHumanoid)

-- Chat
local LINE = 26
local MAX_LINES = 7
local CHAT_W = 400

local log = Instance.new("Frame")
log.Name = "ChatLog"
log.Size = UDim2.new(0, CHAT_W, 0, MAX_LINES * LINE)
log.Position = UDim2.new(0, 12, 1, -(MAX_LINES * LINE) - 64)
log.BackgroundTransparency = 1
log.Parent = gui

local bar, barShadow = card("ChatBar", UDim2.new(0, CHAT_W, 0, 36), UDim2.new(0, 12, 1, -50), gui)
barShadow.Visible = false
local input = Instance.new("TextBox")
input.Name = "Input"
input.Text = ""
input.Size = UDim2.new(1, -24, 1, 0)
input.Position = UDim2.new(0, 12, 0, 0)
input.BackgroundTransparency = 1
input.Font = Enum.Font.SourceSans
input.FontSize = Enum.FontSize.Size18
input.TextColor3 = INK
input.TextXAlignment = Enum.TextXAlignment.Left
input.ClearTextOnFocus = true
input.Parent = bar
local hint = text("Hint", "Type a message, Enter to send", UDim2.new(1, -24, 1, 0), UDim2.new(0, 12, 0, 0),
	Enum.Font.SourceSans, Enum.FontSize.Size18, SLATE, bar)
frame("Focus", UDim2.new(1, 0, 0, 2), UDim2.new(0, 0, 1, -2), BLUE, bar)

local rows = {}
local unreadCount = 0
local chatOpen = false

local function place()
	for i, r in ipairs(rows) do
		local y = (MAX_LINES - (#rows - i + 1)) * LINE
		r:TweenPosition(UDim2.new(0, 0, 0, y), Enum.EasingDirection.Out, Enum.EasingStyle.Quart, 0.2, true)
	end
end

local function addLine(speaker, message, color, system)
	local pill = frame("Line", UDim2.new(0, CHAT_W, 0, LINE - 4), UDim2.new(0, 0, 0, MAX_LINES * LINE), SNOW, log)
	pill.BackgroundTransparency = 0.06
	local nameLabel = text("Speaker", speaker, UDim2.new(0, 140, 1, 0), UDim2.new(0, 10, 0, 0),
		Enum.Font.SourceSansBold, Enum.FontSize.Size18, color, pill)
	local nameWidth = math.min(140, nameLabel.TextBounds.X)
	nameLabel.Size = UDim2.new(0, nameWidth, 1, 0)
	local body = text("Message", message, UDim2.new(0, CHAT_W - nameWidth - 30, 1, 0), UDim2.new(0, nameWidth + 18, 0, 0),
		Enum.Font.SourceSans, Enum.FontSize.Size18, system and SLATE or INK, pill)
	local width = math.min(CHAT_W, nameWidth + 28 + body.TextBounds.X)
	pill.Size = UDim2.new(0, width, 0, LINE - 4)
	table.insert(rows, pill)
	if #rows > MAX_LINES then
		table.remove(rows, 1):Destroy()
	end
	place()
	if not chatOpen and not system then
		unreadCount = unreadCount + 1
		unread.Visible = true
		unreadText.Text = unreadCount > 9 and "9+" or tostring(unreadCount)
	end
end

local function hook(p)
	p.Chatted:connect(function(message)
		addLine(p.Name, message, nameColor(p.Name), false)
	end)
end
for _, p in ipairs(Players:GetPlayers()) do
	hook(p)
end
Players.PlayerAdded:connect(function(p)
	hook(p)
	addLine(p.Name, "joined", SLATE, true)
end)
Players.PlayerRemoving:connect(function(p)
	addLine(p.Name, "left", SLATE, true)
end)

local function setChatOpen(open)
	chatOpen = open
	barShadow.Visible = open
	if open then
		unreadCount = 0
		unread.Visible = false
		input.Text = ""
		hint.Visible = true
		input:CaptureFocus()
	end
end

input.Changed:connect(function(prop)
	if prop == "Text" then
		hint.Visible = input.Text == ""
	end
end)
input.FocusLost:connect(function(enter)
	local message = input.Text
	setChatOpen(false)
	if enter and message ~= "" then
		pcall(function() Players:Chat(message) end)
	end
end)
chatButton.MouseButton1Click:connect(function() setChatOpen(true) end)
UserInputService.InputBegan:connect(function(inputObject, processed)
	if not processed and inputObject.KeyCode == Enum.KeyCode.Slash then
		setChatOpen(true)
	end
end)

addLine("Chat", "press / to talk", SLATE, true)

-- Menu drawer
local DRAWER_W = 340
local shade = frame("Shade", UDim2.new(1, 0, 1, 0), UDim2.new(0, 0, 0, 0), INK, gui)
shade.BackgroundTransparency = 0.55
shade.Active = true
shade.Visible = false

local drawer = frame("Drawer", UDim2.new(0, DRAWER_W, 1, 0), UDim2.new(0, -DRAWER_W, 0, 0), SNOW, shade)
text("Title", "Paused", UDim2.new(1, -48, 0, 36), UDim2.new(0, 24, 0, 28),
	Enum.Font.SourceSansBold, Enum.FontSize.Size32, INK, drawer)
text("Who", "Playing as " .. player.Name, UDim2.new(1, -48, 0, 22), UDim2.new(0, 24, 0, 68),
	Enum.Font.SourceSans, Enum.FontSize.Size18, SLATE, drawer)

local resume = Instance.new("TextButton")
resume.Name = "Resume"
resume.Text = "Resume"
resume.Size = UDim2.new(1, -48, 0, 44)
resume.Position = UDim2.new(0, 24, 0, 112)
resume.BackgroundColor3 = BLUE
resume.BorderSizePixel = 0
resume.AutoButtonColor = false
resume.Font = Enum.Font.SourceSansBold
resume.FontSize = Enum.FontSize.Size18
resume.TextColor3 = SNOW
resume.Parent = drawer
resume.MouseEnter:connect(function() resume.BackgroundColor3 = Color3.new(0, 136/255, 220/255) end)
resume.MouseLeave:connect(function() resume.BackgroundColor3 = BLUE end)

local function row(name, label, image, y, color)
	local b = Instance.new("TextButton")
	b.Name = name
	b.Text = ""
	b.Size = UDim2.new(1, 0, 0, 52)
	b.Position = UDim2.new(0, 0, 0, y)
	b.BackgroundColor3 = SNOW
	b.BorderSizePixel = 0
	b.AutoButtonColor = false
	b.Parent = drawer
	icon("Glyph", image, UDim2.new(0, 22, 0, 22), UDim2.new(0, 24, 0.5, -11), color, b)
	text("Label", label, UDim2.new(1, -80, 1, 0), UDim2.new(0, 60, 0, 0),
		Enum.Font.SourceSans, Enum.FontSize.Size18, color, b)
	frame("Rule", UDim2.new(1, -48, 0, 1), UDim2.new(0, 24, 1, -1), RULE, b)
	b.MouseEnter:connect(function() b.BackgroundColor3 = MIST end)
	b.MouseLeave:connect(function() b.BackgroundColor3 = SNOW end)
	return b
end

local resetRow = row("Reset", "Reset character", ICON_RESET, 180, INK)
local playersRow = row("Players", tostring(#Players:GetPlayers()) .. " in this game", ICON_PLAYERS, 232, INK)
local leaveRow = row("Leave", "Leave game", ICON_LEAVE, 284, CORAL)
text("Hint", "Esc to resume", UDim2.new(1, -48, 0, 22), UDim2.new(0, 24, 1, -46),
	Enum.Font.SourceSans, Enum.FontSize.Size18, SLATE, drawer)

local function raise(object, z)
	if object:IsA("GuiObject") then
		object.ZIndex = z
	end
	for _, child in ipairs(object:GetChildren()) do
		raise(child, z)
	end
end
raise(shade, 8)
for _, r in ipairs({resetRow, playersRow, leaveRow}) do
	r.Glyph.ZIndex = 9
	r.Label.ZIndex = 9
	r.Rule.ZIndex = 9
end

local menuOpen = false
local function setMenu(open)
	menuOpen = open
	pcall(function() GuiService:SetMenuIsOpen(open) end)
	if open then
		playersRow.Label.Text = tostring(#Players:GetPlayers()) .. " in this game"
		shade.Visible = true
		drawer.Position = UDim2.new(0, -DRAWER_W, 0, 0)
		drawer:TweenPosition(UDim2.new(0, 0, 0, 0), Enum.EasingDirection.Out, Enum.EasingStyle.Quart, 0.22, true)
	else
		drawer:TweenPosition(UDim2.new(0, -DRAWER_W, 0, 0), Enum.EasingDirection.In, Enum.EasingStyle.Quad, 0.14, true,
			function() if not menuOpen then shade.Visible = false end end)
	end
end

resume.MouseButton1Click:connect(function() setMenu(false) end)
resetRow.MouseButton1Click:connect(function()
	setMenu(false)
	local humanoid = player.Character and player.Character:FindFirstChild("Humanoid")
	if humanoid then humanoid.Health = 0 end
end)
leaveRow.MouseButton1Click:connect(function()
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
	UserInputService.InputBegan:connect(function(inputObject)
		if inputObject.KeyCode == Enum.KeyCode.Escape then
			setMenu(not menuOpen)
		end
	end)
end

if _G.ExperimentalUiShowMenu then
	setMenu(true)
end
